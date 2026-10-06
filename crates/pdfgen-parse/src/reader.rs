//! The reader: xref loading (classic, stream, hybrid), lazy object
//! resolution, and repair mode for damaged files.

use crate::{Lexer, ParseError, Token};
use pdfgen_core::{Dict, Name, Object, PdfString, Real, Ref, Stream};
use std::collections::HashMap;
use std::path::Path;

/// One xref entry.
#[derive(Debug, Clone, Copy)]
enum Entry {
    /// In use at this byte offset.
    InUse(u64),
    /// Compressed inside an object stream: (stream object id, index).
    Compressed { stream: u32, index: u32 },
    /// Free.
    Free,
}

/// Information about a repair pass, when one happened.
#[derive(Debug, Clone, Default)]
pub struct RepairInfo {
    /// True when the xref was unusable and objects were re-found by scanning.
    pub scanned: bool,
    /// Number of objects recovered.
    pub recovered: usize,
}

/// A parsed PDF file.
pub struct PdfReader {
    data: Vec<u8>,
    xref: HashMap<u32, Entry>,
    trailer: Dict,
    /// Repair details, if a repair pass ran.
    pub repair: RepairInfo,
    /// Cache of already-resolved top-level objects.
    cache: HashMap<u32, Object>,
    /// Object-stream contents cache: stream id -> its /N decoded objects.
    objstm_cache: HashMap<u32, Vec<Object>>,
    /// Objects currently being resolved (cycle protection).
    resolving: Vec<u32>,
    /// Object numbers known to exist (sorted).
    known: Vec<u32>,
}

impl PdfReader {
    /// Parse a file from disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ParseError> {
        Self::from_bytes(std::fs::read(path)?)
    }

    /// Parse from in-memory bytes.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, ParseError> {
        let mut reader = PdfReader {
            data,
            xref: HashMap::new(),
            trailer: Dict::new(),
            repair: RepairInfo::default(),
            cache: HashMap::new(),
            objstm_cache: HashMap::new(),
            resolving: Vec::new(),
            known: Vec::new(),
        };
        reader.check_header()?;
        reader.load_xref().or_else(|_| reader.repair())?;
        Ok(reader)
    }

    fn check_header(&self) -> Result<(), ParseError> {
        let head = &self.data[..self.data.len().min(1024)];
        let text = String::from_utf8_lossy(head);
        if text.contains("%PDF-1.") || text.contains("%PDF-2.") {
            Ok(())
        } else {
            Err(ParseError::BadHeader)
        }
    }

    /// Locate the last `startxref` value.
    fn last_startxref(&self) -> Option<u64> {
        let tail_start = self.data.len().saturating_sub(2048);
        let tail = &self.data[tail_start..];
        let mut idx = None;
        let mut i = 0;
        while i + 9 <= tail.len() {
            if &tail[i..i + 9] == b"startxref" {
                idx = Some(tail_start + i);
            }
            i += 1;
        }
        let pos = idx?;
        let mut lx = Lexer::new(&self.data);
        lx.seek(pos + 9);
        match lx.next_token().ok()? {
            Some(Token::Int(offset)) if offset >= 0 => Some(offset as u64),
            _ => None,
        }
    }

    fn load_xref(&mut self) -> Result<(), ParseError> {
        let Some(offset) = self.last_startxref() else {
            return Err(ParseError::NoXref);
        };
        if offset as usize >= self.data.len() {
            return Err(ParseError::BadXrefOffset(offset));
        }
        let mut lx = Lexer::new(&self.data);
        lx.seek(offset as usize);
        match lx.peek() {
            Some(b'x') => self.load_classic_xref(offset as usize),
            Some(b'0'..=b'9') => self.load_xref_stream(offset as usize),
            _ => Err(ParseError::BadXrefOffset(offset)),
        }?;

        // --- Follow the /Prev chain ---------------------------------
        // Incremental updates append newer xref sections linked by /Prev;
        // older entries fill the gaps the newest section doesn't cover.
        // The newest entry for an object wins (it was inserted first; we
        // only insert when absent).
        let mut visited: std::collections::HashSet<u64> = std::collections::HashSet::new();
        loop {
            let prev = match self.trailer.get("Prev") {
                Some(Object::Int(p)) => *p as u64,
                _ => break,
            };
            if prev as usize >= self.data.len() || !visited.insert(prev) {
                break; // out of range or a loop: stop, keep what we have
            }
            // Remember which keys the newest trailer lacks; older
            // sections may provide them (e.g. the first /ID, /Info).
            let saved_trailer = std::mem::take(&mut self.trailer);
            self.trailer = pdfgen_core::Dict::new();
            let mut probe = Lexer::new(&self.data);
            probe.seek(prev as usize);
            match probe.peek() {
                Some(b'x') => self.load_classic_xref(prev as usize)?,
                Some(b'0'..=b'9') => self.load_xref_stream(prev as usize)?,
                _ => {
                    self.trailer = saved_trailer;
                    break;
                }
            }
            // Merge: keep the newest section's keys, add any older keys
            // the newest lacked (except Prev itself, which is chain state).
            let older = std::mem::take(&mut self.trailer);
            self.trailer = saved_trailer;
            for (k, v) in older.0 {
                if k.0 != "Prev" && self.trailer.get(&k.0).is_none() {
                    self.trailer.set(k.0.clone(), v);
                }
            }
        }
        self.rebuild_known();
        Ok(())
    }

    fn load_classic_xref(&mut self, offset: usize) -> Result<(), ParseError> {
        {
            let mut lx = Lexer::new(&self.data);
            lx.seek(offset);
            match lx.next_token()? {
                Some(Token::XrefKeyword) => {}
                _ => return Err(ParseError::BadXrefOffset(offset as u64)),
            }
            loop {
                match lx.peek() {
                    Some(b't') | None => break,
                    _ => {}
                }
                let Some(Token::Int(first)) = lx.next_token()? else {
                    break;
                };
                let Some(Token::Int(count)) = lx.next_token()? else {
                    break;
                };
                for i in 0..count {
                    lx.skip_trivia();
                    let start = lx.pos();
                    let rec_len = 20.min(self.data.len() - start);
                    if rec_len < 18 {
                        break;
                    }
                    let rec = &self.data[start..start + rec_len];
                    let text = String::from_utf8_lossy(rec);
                    let nums: Vec<&str> = text.split_whitespace().collect();
                    if nums.len() < 3 {
                        break;
                    }
                    let Ok(off) = nums[0].parse::<u64>() else {
                        break;
                    };
                    let id = u32::try_from(first + i).unwrap_or(0);
                    if nums[2].starts_with('n') {
                        self.xref.entry(id).or_insert(Entry::InUse(off));
                    } else {
                        self.xref.entry(id).or_insert(Entry::Free);
                    }
                    lx.seek(start + 20);
                }
            }
            // trailer dict
            match lx.next_token()? {
                Some(Token::TrailerKeyword) => {}
                _ => return Err(ParseError::Malformed(lx.pos(), "expected trailer".into())),
            }
            let Some(Token::DictOpen) = lx.next_token()? else {
                return Err(ParseError::Malformed(
                    lx.pos(),
                    "expected trailer dict".into(),
                ));
            };
            let open = lx.pos() - 2;
            self.trailer = parse_dict(&self.data, &mut lx, open)?;
        }
        // Hybrid file: an /XRefStm pointer to an xref stream.
        if let Some(Object::Int(stm)) = self.trailer.get("XRefStm") {
            self.load_xref_stream(*stm as usize)?;
        }
        self.rebuild_known();
        Ok(())
    }

    fn load_xref_stream(&mut self, offset: usize) -> Result<(), ParseError> {
        let dict = {
            let mut lx = Lexer::new(&self.data);
            lx.seek(offset);
            let Some(Token::Int(_)) = lx.next_token()? else {
                return Err(ParseError::BadXrefOffset(offset as u64));
            };
            let Some(Token::Int(_)) = lx.next_token()? else {
                return Err(ParseError::BadXrefOffset(offset as u64));
            };
            let Some(Token::ObjKeyword) = lx.next_token()? else {
                return Err(ParseError::BadXrefOffset(offset as u64));
            };
            let Some(Token::DictOpen) = lx.next_token()? else {
                return Err(ParseError::Malformed(offset, "expected <<".into()));
            };
            let open = lx.pos() - 2;
            parse_dict(&self.data, &mut lx, open)?
        };

        // Read stream bytes.
        let (body_start, end) = {
            let mut lx = Lexer::new(&self.data);
            lx.seek(offset);
            let body = find_stream_body(&self.data, &mut lx)?;
            let end = find_endstream(&self.data, body)?;
            (body, end)
        };
        // /Length may be an indirect reference (common in the wild).
        let declared = match dict.get("Length") {
            Some(Object::Int(l)) => *l as usize,
            Some(Object::Ref(r)) => match self.get(r.id) {
                Ok(Object::Int(l)) => l as usize,
                _ => end - body_start,
            },
            _ => end - body_start,
        };
        let raw = &self.data[body_start..(body_start + declared).min(end)];

        // FlateDecode, via single name or a /Filter array.
        let filtered = matches!(dict.get("Filter"), Some(Object::Name(n)) if n.0 == "FlateDecode")
            || matches!(
                dict.get("Filter"),
                Some(Object::Array(items)) if items.iter().any(|o| matches!(o, Object::Name(n) if n.0 == "FlateDecode"))
            );
        let mut bytes: Vec<u8> = if filtered {
            inflate(raw)
                .ok_or_else(|| ParseError::Malformed(offset, "zlib decode failed".into()))?
        } else {
            raw.to_vec()
        };

        // PNG predictors (DecodeParms): /Predictor 12+ is nearly always
        // present on real-world xref streams.
        if let Some(parms) = dict.get("DecodeParms") {
            match parms {
                Object::Dict(parms) => {
                    bytes = apply_predictors(bytes, parms)?;
                }
                Object::Array(parr) => {
                    // One parms dict per filter; apply the first that
                    // asks for a PNG predictor.
                    for p in parr {
                        if let Object::Dict(parms) = p {
                            bytes = apply_predictors(bytes, parms)?;
                            break;
                        }
                    }
                }
                _ => {}
            }
        }

        let w: Vec<usize> = match dict.get("W") {
            Some(Object::Array(items)) => items
                .iter()
                .filter_map(|o| match o {
                    Object::Int(v) => Some(*v as usize),
                    _ => None,
                })
                .collect(),
            _ => {
                return Err(ParseError::Malformed(
                    offset,
                    "xref stream without /W".into(),
                ))
            }
        };
        let size = match dict.get("Size") {
            Some(Object::Int(s)) => *s as u32,
            _ => 0,
        };
        let mut index: Vec<(u32, u32)> = match dict.get("Index") {
            Some(Object::Array(items)) => items
                .chunks(2)
                .filter_map(|c| match (&c.first(), &c.get(1)) {
                    (Some(Object::Int(a)), Some(Object::Int(b))) => Some((*a as u32, *b as u32)),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        if index.is_empty() {
            index = vec![(0, size)];
        }
        let row: usize = w.iter().sum();
        let mut pos = 0usize;
        for (first, count) in index {
            for i in 0..count {
                if pos + row > bytes.len() {
                    break;
                }
                let mut fields = [0u64; 3];
                for (f, width) in w.iter().enumerate() {
                    let mut v = 0u64;
                    for k in 0..*width {
                        v = (v << 8) | u64::from(bytes[pos + k]);
                    }
                    fields[f.min(2)] = v;
                    pos += width;
                }
                let id = first + i;
                match fields[0] {
                    0 => {
                        self.xref.entry(id).or_insert(Entry::Free);
                    }
                    1 => {
                        self.xref.entry(id).or_insert(Entry::InUse(fields[1]));
                    }
                    2 => {
                        // Compressed object in an object stream; field1 is
                        // the object-stream number, field2 the index in it.
                        self.xref.entry(id).or_insert(Entry::Compressed {
                            stream: fields[1] as u32,
                            index: fields[2] as u32,
                        });
                    }
                    _ => {}
                }
            }
        }
        for (k, v) in dict.0 {
            if matches!(k.0.as_str(), "Root" | "Info" | "ID" | "Size") {
                self.trailer.set(k.0, v);
            }
        }
        self.rebuild_known();
        Ok(())
    }

    fn rebuild_known(&mut self) {
        self.known = self.xref.keys().copied().collect();
        self.known.sort_unstable();
    }

    /// Repair mode: scan the whole file for `N G obj` headers when the xref
    /// is missing or unusable. First declaration of each object wins.
    fn repair(&mut self) -> Result<(), ParseError> {
        let mut recovered = 0usize;
        let mut pos = 0usize;
        while pos + 4 < self.data.len() {
            if &self.data[pos..pos + 3] == b"obj"
                && (pos + 3 >= self.data.len() || is_ws(self.data[pos + 3]))
            {
                let mut p = pos;
                let mut nums = Vec::new();
                for _ in 0..3 {
                    p = walk_back_ws(&self.data, p);
                    let (np, n) = walk_back_number(&self.data, p);
                    let Some(n) = n else { break };
                    nums.push(n);
                    p = np;
                }
                if nums.len() == 2 {
                    // nums[0] = generation, nums[1] = object id; `p` now
                    // points at the START of the "N G obj" header.
                    let id = u32::try_from(nums[1]).unwrap_or(0);
                    if !self.xref.contains_key(&id) {
                        recovered += 1;
                    }
                    self.xref.entry(id).or_insert(Entry::InUse(p as u64));
                }
            }
            pos += 1;
        }
        if self.xref.is_empty() {
            return Err(ParseError::NoXref);
        }
        // Find the LAST trailer dict.
        let mut trailer_pos = None;
        let mut i = 0usize;
        while i + 7 < self.data.len() {
            if &self.data[i..i + 7] == b"trailer" {
                trailer_pos = Some(i);
            }
            i += 1;
        }
        if let Some(tp) = trailer_pos {
            let mut lx = Lexer::new(&self.data);
            lx.seek(tp + 7);
            if let Some(Token::DictOpen) = lx.next_token()? {
                let open = lx.pos() - 2;
                self.trailer = parse_dict(&self.data, &mut lx, open)?;
            }
        }
        self.repair = RepairInfo {
            scanned: true,
            recovered,
        };
        self.rebuild_known();
        Ok(())
    }

    /// All object numbers known to the reader (sorted).
    pub fn object_ids(&self) -> &[u32] {
        &self.known
    }

    /// Known object offsets: (id, byte offset) for every in-use entry.
    /// Used by the revision crate to re-emit xref tables.
    pub fn object_offsets(&self) -> Vec<(u32, u64)> {
        let mut out: Vec<(u32, u64)> = self
            .xref
            .iter()
            .filter_map(|(id, e)| match e {
                Entry::InUse(off) => Some((*id, *off)),
                Entry::Compressed { .. } | Entry::Free => None,
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// Resolve an object by number (lazily; cached).
    pub fn get(&mut self, id: u32) -> Result<Object, ParseError> {
        if let Some(cached) = self.cache.get(&id) {
            return Ok(cached.clone());
        }
        if self.resolving.contains(&id) {
            return Ok(Object::Null); // cycle guard
        }
        match self.xref.get(&id).copied() {
            Some(Entry::InUse(_)) => {}
            Some(Entry::Compressed { stream, index }) => {
                return self.get_from_object_stream(stream, index);
            }
            _ => return Err(ParseError::ObjectNotFound(id)),
        }
        let offset = match self.xref.get(&id).copied() {
            Some(Entry::InUse(off)) => off,
            _ => unreachable!("checked above"),
        };
        self.resolving.push(id);
        let result = self.parse_object_at(id, offset);
        self.resolving.pop();
        let obj = result?;
        self.cache.insert(id, obj.clone());
        Ok(obj)
    }

    /// Resolve an object compressed inside an object stream (PDF 1.5+).
    /// `stream` is the object-stream's object id; `index` selects which
    /// of its /N objects to return. Stream objects are cached whole.
    fn get_from_object_stream(&mut self, stream: u32, index: u32) -> Result<Object, ParseError> {
        if let Some(cached) = self.objstm_cache.get(&stream) {
            if let Some(obj) = cached.get(index as usize) {
                return Ok(obj.clone());
            }
            return Err(ParseError::ObjectNotFound(stream));
        }
        // Parse the object stream itself (by normal xref entry).
        let raw = match self.xref.get(&stream).copied() {
            Some(Entry::InUse(off)) => self.parse_object_at(stream, off)?,
            _ => return Err(ParseError::ObjectNotFound(stream)),
        };
        let Object::Stream(stm) = raw else {
            return Err(ParseError::Malformed(
                0,
                "object stream is not a stream".into(),
            ));
        };
        let n = match stm.dict.get("N") {
            Some(Object::Int(n)) => *n as u32,
            _ => return Err(ParseError::Malformed(0, "object stream missing /N".into())),
        };
        let first = match stm.dict.get("First") {
            Some(Object::Int(f)) => *f as usize,
            _ => {
                return Err(ParseError::Malformed(
                    0,
                    "object stream missing /First".into(),
                ))
            }
        };
        // Decode the stream data (FlateDecode and filter arrays).
        let data = self.decode_stream_data(&stm)?;
        if first >= data.len() {
            return Err(ParseError::Malformed(
                0,
                "object stream /First out of range".into(),
            ));
        }
        // Header: N pairs of (obj id, relative offset) in the decoded data.
        let header = String::from_utf8_lossy(&data[..first]);
        let nums: Vec<u32> = header
            .split_whitespace()
            .filter_map(|t| t.parse::<u32>().ok())
            .collect();
        if nums.len() < (n as usize) * 2 {
            return Err(ParseError::Malformed(
                0,
                "object stream header truncated".into(),
            ));
        }
        let mut objects: Vec<Object> = Vec::with_capacity(n as usize);
        for i in 0..n as usize {
            // Header layout: N pairs of (obj id, relative offset), so
            // offset i lives at nums[2*i + 1].
            let rel = nums.get(2 * i + 1).copied().unwrap_or(0) as usize;
            let abs = first + rel;
            if abs >= data.len() {
                objects.push(Object::Null);
                continue;
            }
            let mut lx = Lexer::new(&data);
            lx.seek(abs);
            let obj = match parse_value(&data, &mut lx) {
                Ok(Some(o)) => o,
                Ok(None) | Err(_) => Object::Null,
            };
            objects.push(obj);
        }
        self.objstm_cache.insert(stream, objects);
        let obj = self
            .objstm_cache
            .get(&stream)
            .and_then(|c| c.get(index as usize).cloned());
        obj.ok_or_else(|| ParseError::ObjectNotFound(stream))
    }

    /// Decode a stream's bytes through its /Filter (single name or array),
    /// with FlateDecode support. Predictors are applied for xref streams
    /// by the caller that needs them.
    fn decode_stream_data(&self, stm: &Stream) -> Result<Vec<u8>, ParseError> {
        match stm.dict.get("Filter") {
            Some(Object::Name(n)) if n.0 == "FlateDecode" => inflate(&stm.data)
                .ok_or_else(|| ParseError::Malformed(0, "zlib decode failed".into())),
            Some(Object::Array(filters)) => {
                // Filter arrays: apply the filters in order. Only
                // FlateDecode is implemented; unknown filters pass the
                // data through (best effort, like most readers).
                let mut data = stm.data.clone();
                for f in filters {
                    if let Object::Name(n) = f {
                        if n.0 == "FlateDecode" {
                            data = inflate(&data).ok_or_else(|| {
                                ParseError::Malformed(0, "zlib decode failed".into())
                            })?;
                        }
                    }
                }
                Ok(data)
            }
            _ => Ok(stm.data.clone()),
        }
    }

    fn parse_object_at(&mut self, id: u32, offset: u64) -> Result<Object, ParseError> {
        let data = &self.data;
        let mut lx = Lexer::new(data);
        lx.seek(offset as usize);
        // "N G obj"
        match lx.next_token()? {
            Some(Token::Int(n)) if n as u32 == id => {}
            _ => {
                return Err(ParseError::Malformed(
                    offset as usize,
                    format!("object {id}: bad header at offset {offset}"),
                ));
            }
        }
        match lx.next_token()? {
            Some(Token::Int(_)) => {}
            _ => {
                return Err(ParseError::Malformed(
                    offset as usize,
                    "missing generation".into(),
                ))
            }
        }
        match lx.next_token()? {
            Some(Token::ObjKeyword) => {}
            _ => {
                return Err(ParseError::Malformed(
                    offset as usize,
                    "missing obj keyword".into(),
                ))
            }
        }
        // Body: dict (+stream), array, or scalar.
        match lx.peek() {
            Some(b'<') => {
                lx.next_token()?; // DictOpen
                let open = lx.pos() - 2;
                let dict = parse_dict(data, &mut lx, open)?;
                // Stream body?
                let save = lx.pos();
                lx.skip_trivia();
                if matches!(lx.next_token()?, Some(Token::StreamKeyword)) {
                    // Trust /Length when present: binary streams may contain
                    // the bytes "endstream" inside their data.
                    let mut body_start = lx.pos();
                    if data.get(body_start) == Some(&b'\r')
                        && data.get(body_start + 1) == Some(&b'\n')
                    {
                        body_start += 2;
                    } else if data.get(body_start) == Some(&b'\n') {
                        body_start += 1;
                    }
                    let end = match dict.get("Length") {
                        Some(Object::Int(l)) if (body_start + *l as usize) <= data.len() => {
                            let e = body_start + *l as usize;
                            // Sanity: endstream must follow (after EOL).
                            let mut probe = e;
                            if data.get(probe) == Some(&b'\r') {
                                probe += 1;
                            }
                            if data.get(probe) == Some(&b'\n') {
                                probe += 1;
                            }
                            if data[probe..].starts_with(b"endstream") {
                                e
                            } else {
                                // /Length lied; fall back to scanning.
                                find_endstream(data, body_start)?
                            }
                        }
                        _ => find_endstream(data, body_start)?,
                    };
                    let raw = data[body_start..end].to_vec();
                    return Ok(Object::Stream(Stream { dict, data: raw }));
                }
                lx.seek(save);
                Ok(Object::Dict(dict))
            }
            _ => {
                let Some(v) = parse_value(data, &mut lx)? else {
                    return Err(ParseError::Malformed(
                        offset as usize,
                        "object body missing".into(),
                    ));
                };
                Ok(v)
            }
        }
    }

    /// The trailer dictionary (catalog `/Root`, `/Info`, `/ID`, …).
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// The catalog dictionary (`/Root` in the trailer).
    /// All page object ids in document order, walking the page tree with
    /// cycle protection (visited set + depth limit) so malicious trees
    /// produce errors instead of stack overflows.
    pub fn pages(&mut self) -> Result<Vec<u32>, ParseError> {
        let catalog = self.catalog()?;
        let Some(Object::Ref(pages_ref)) = catalog.get("Pages").cloned() else {
            return Err(ParseError::Malformed(0, "catalog has no /Pages".into()));
        };
        let mut out = Vec::new();
        let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        self.walk_page_node(pages_ref.id, &mut out, &mut visited, 0)?;
        Ok(out)
    }

    /// Depth-limited page-tree walk. Leaves (no /Kids) are pages; internal
    /// nodes recurse into /Kids. Cycles and excessive depth stop the walk.
    fn walk_page_node(
        &mut self,
        node_id: u32,
        out: &mut Vec<u32>,
        visited: &mut std::collections::HashSet<u32>,
        depth: usize,
    ) -> Result<(), ParseError> {
        const MAX_DEPTH: usize = 64;
        if depth > MAX_DEPTH || !visited.insert(node_id) {
            // Cycle or runaway depth: stop this branch.
            return Ok(());
        }
        let node = self.get(node_id)?;
        if let Object::Dict(d) = node {
            match d.get("Kids").cloned() {
                Some(Object::Array(kids)) => {
                    for k in kids {
                        if let Object::Ref(r) = k {
                            self.walk_page_node(r.id, out, visited, depth + 1)?;
                        }
                    }
                }
                _ => out.push(node_id),
            }
        }
        Ok(())
    }

    /// The document catalog dictionary (from the trailer /Root).
    pub fn catalog(&mut self) -> Result<Dict, ParseError> {
        let Some(Object::Ref(root)) = self.trailer.get("Root").cloned() else {
            return Err(ParseError::Malformed(0, "trailer has no /Root".into()));
        };
        match self.get(root.id)? {
            Object::Dict(d) => Ok(d),
            _ => Err(ParseError::Malformed(
                0,
                "catalog is not a dictionary".into(),
            )),
        }
    }
}

// --- Free functions over (data, lexer) to satisfy the borrow checker ------

fn parse_dict(data: &[u8], lx: &mut Lexer<'_>, open_pos: usize) -> Result<Dict, ParseError> {
    let mut dict = Dict::new();
    loop {
        match lx.next_token()? {
            Some(Token::Name(key)) => {
                let Some(value) = parse_value(data, lx)? else {
                    return Err(ParseError::Malformed(open_pos, "dict value missing".into()));
                };
                dict.set(key, value);
            }
            Some(Token::DictClose) => break,
            None => return Err(ParseError::Malformed(open_pos, "unterminated dict".into())),
            Some(t) => {
                return Err(ParseError::Malformed(
                    open_pos,
                    format!("unexpected token in dict: {t:?}"),
                ));
            }
        }
    }
    Ok(dict)
}

fn parse_value(data: &[u8], lx: &mut Lexer<'_>) -> Result<Option<Object>, ParseError> {
    match lx.next_token()? {
        Some(Token::Int(a)) => {
            let save = lx.pos();
            match lx.next_token()? {
                Some(Token::Int(_gen)) => match lx.next_token()? {
                    Some(Token::RefKeyword) => {
                        Ok(Some(Object::Ref(Ref::new(u32::try_from(a).unwrap_or(0)))))
                    }
                    _ => {
                        lx.seek(save);
                        Ok(Some(Object::Int(a)))
                    }
                },
                _ => {
                    lx.seek(save);
                    Ok(Some(Object::Int(a)))
                }
            }
        }
        Some(Token::Real(v)) => Ok(Some(Object::Real(Real(v)))),
        Some(Token::Name(n)) => Ok(Some(Object::Name(Name(n)))),
        Some(Token::LiteralString(s)) => Ok(Some(Object::String(PdfString(s)))),
        Some(Token::HexString(s)) => Ok(Some(Object::String(PdfString(s)))),
        Some(Token::True) => Ok(Some(Object::Bool(true))),
        Some(Token::False) => Ok(Some(Object::Bool(false))),
        Some(Token::Null) => Ok(Some(Object::Null)),
        Some(Token::ArrayOpen) => {
            let mut items = Vec::new();
            loop {
                match lx.peek() {
                    Some(b']') => {
                        lx.next_token()?;
                        break;
                    }
                    None => {
                        return Err(ParseError::Malformed(lx.pos(), "unterminated array".into()))
                    }
                    _ => {}
                }
                if let Some(v) = parse_value(data, lx)? {
                    items.push(v);
                } else {
                    break;
                }
            }
            Ok(Some(Object::Array(items)))
        }
        Some(Token::DictOpen) => {
            let open = lx.pos() - 2;
            Ok(Some(Object::Dict(parse_dict(data, lx, open)?)))
        }
        None => Ok(None),
        Some(t) => Err(ParseError::Malformed(
            lx.pos(),
            format!("unexpected value token {t:?}"),
        )),
    }
}

fn find_stream_body(data: &[u8], lx: &mut Lexer<'_>) -> Result<usize, ParseError> {
    let mut probe = lx.pos();
    while probe + 7 <= data.len() {
        if &data[probe..probe + 6] == b"stream" {
            probe += 6;
            if data.get(probe) == Some(&b'\r') {
                probe += 1;
            }
            if data.get(probe) == Some(&b'\n') {
                probe += 1;
            }
            lx.seek(probe);
            return Ok(probe);
        }
        probe += 1;
    }
    Err(ParseError::Malformed(
        lx.pos(),
        "stream keyword not found".into(),
    ))
}

fn find_endstream(data: &[u8], from: usize) -> Result<usize, ParseError> {
    let mut i = from;
    while i + 9 <= data.len() {
        if &data[i..i + 9] == b"endstream" {
            let mut end = i;
            while end > from && (data[end - 1] == b'\n' || data[end - 1] == b'\r') {
                end -= 1;
            }
            return Ok(end);
        }
        i += 1;
    }
    Err(ParseError::Malformed(from, "endstream not found".into()))
}

/// Apply PNG predictors to a decoded stream per its /DecodeParms.
/// Columns default to 1; /Predictor 2 = TIFF (unsupported, pass through);
/// 10..=15 = PNG predictors with the per-row filter byte.
fn apply_predictors(data: Vec<u8>, parms: &Dict) -> Result<Vec<u8>, ParseError> {
    let predictor = match parms.get("Predictor") {
        Some(Object::Int(p)) => *p as i64,
        _ => return Ok(data),
    };
    if predictor < 10 {
        // No PNG prediction (TIFF predictor 2 is not needed for xref
        // streams in practice); leave the data as-is.
        return Ok(data);
    }
    let colors = match parms.get("Colors") {
        Some(Object::Int(c)) => (*c).max(1) as usize,
        _ => 1,
    };
    let bpc = match parms.get("BitsPerComponent") {
        Some(Object::Int(b)) => (*b).max(1) as usize,
        _ => 8,
    };
    let columns = match parms.get("Columns") {
        Some(Object::Int(c)) => (*c).max(1) as usize,
        _ => 1,
    };
    let bpp = colors * bpc / 8; // bytes per pixel (1 for xref streams)
    let row_len = colors * bpc * columns / 8;
    if row_len == 0 || data.len() % (row_len + 1) != 0 {
        // Not a clean predictor layout; best effort: return the raw data
        // (callers treat xref failure as a repair-scan path).
        return Ok(data);
    }
    let rows = data.len() / (row_len + 1);
    let mut out: Vec<u8> = Vec::with_capacity(rows * row_len);
    let mut prev: Vec<u8> = vec![0; row_len];
    for r in 0..rows {
        let base = r * (row_len + 1);
        let filter = data[base];
        let row = &data[base + 1..base + 1 + row_len];
        let mut cur: Vec<u8> = row.to_vec();
        match filter {
            0 => {} // None
            1 => {
                // Sub
                for i in bpp..row_len {
                    cur[i] = cur[i].wrapping_add(cur[i - bpp]);
                }
            }
            2 => {
                // Up
                for i in 0..row_len {
                    cur[i] = cur[i].wrapping_add(prev[i]);
                }
            }
            3 => {
                // Average
                for i in 0..row_len {
                    let left = if i >= bpp { cur[i - bpp] as u16 } else { 0 };
                    cur[i] = cur[i].wrapping_add(((left + prev[i] as u16) / 2) as u8);
                }
            }
            4 => {
                // Paeth
                for i in 0..row_len {
                    let a = if i >= bpp { cur[i - bpp] as i16 } else { 0 };
                    let b = prev[i] as i16;
                    let c = if i >= bpp { prev[i - bpp] as i16 } else { 0 };
                    let p = a + b - c;
                    let pa = (p - a).abs();
                    let pb = (p - b).abs();
                    let pc = (p - c).abs();
                    let pred = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    } as u8;
                    cur[i] = cur[i].wrapping_add(pred);
                }
            }
            _ => {}
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(out)
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut out = Vec::new();
    ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .ok()
        .map(|_| out)
}

fn is_ws(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | b' ')
}

fn walk_back_ws(data: &[u8], mut p: usize) -> usize {
    while p > 0 && is_ws(data[p - 1]) {
        p -= 1;
    }
    p
}

fn walk_back_number(data: &[u8], p: usize) -> (usize, Option<i64>) {
    let mut e = p;
    while e > 0 && data[e - 1].is_ascii_digit() {
        e -= 1;
    }
    if e == p {
        return (p, None);
    }
    let text = std::str::from_utf8(&data[e..p]).unwrap_or("");
    (e, text.parse::<i64>().ok())
}
