//! The nested structure tree model shared by flow, page and retag.
//!
//! Blocks become a tree: `Table` holds `TR` rows holding `TH`/`TD` cells;
//! `L` lists hold `LI` items holding `Lbl`/`LBody`. Each leaf carries its
//! marked-content pieces (page, MCID) so a node split across pages stays
//! one element via MCR dictionaries.

/// One node of the structure tree.
#[derive(Debug, Clone)]
pub struct Node {
    /// PDF structure type: `H1`..`H6`, `P`, `Table`, `TR`, `TH`, `TD`,
    /// `L`, `LI`, `Lbl`, `LBody`, `Figure`, …
    pub tag: String,
    /// Child nodes (empty for leaves).
    pub children: Vec<Node>,
    /// Leaf content pieces: (page index, MCID). Empty for group nodes.
    pub pieces: Vec<(usize, u32)>,
    /// Marked as artifact instead of real content?
    pub artifact: bool,
    /// Artifact subtype (`Header`, `Footer`) when `artifact` is set.
    /// Alt text for figures and images (PDF/UA 13-004).
    pub alt: Option<String>,
    /// Table cell scope: `Column`, `Row`, or `Both` (PDF/UA 15-003).
    pub scope: String,
    /// Index of the font in the document's font list (leaves only).
    pub font: usize,
    /// Font size in points (leaves only).
    pub size: f64,
    /// Original text (leaves only, diagnostics and bookmarks).
    pub text: String,
}

impl Node {
    /// New leaf node.
    pub fn leaf(tag: impl Into<String>, text: String, font: usize, size: f64) -> Self {
        Node {
            tag: tag.into(),
            children: Vec::new(),
            pieces: Vec::new(),
            artifact: false,
            alt: None,
            scope: String::new(),
            font,
            size,
            text,
        }
    }

    /// New group node.
    pub fn group(tag: impl Into<String>) -> Self {
        Node {
            tag: tag.into(),
            children: Vec::new(),
            pieces: Vec::new(),
            artifact: false,
            alt: None,
            scope: String::new(),
            font: 0,
            size: 0.0,
            text: String::new(),
        }
    }

    /// Attach marked-content pieces (leaves).
    #[must_use]
    pub fn with_pieces(mut self, pieces: Vec<(usize, u32)>) -> Self {
        self.pieces = pieces;
        self
    }

    /// Attach children (group nodes).
    #[must_use]
    pub fn with_children(mut self, children: Vec<Node>) -> Self {
        self.children = children;
        self
    }

    /// Set the table-cell Scope attribute.
    #[must_use]
    pub fn with_scope(mut self, scope: &str) -> Self {
        self.scope = scope.to_string();
        self
    }

    /// Attach alt text (figures).
    #[must_use]
    pub fn with_alt(mut self, alt: Option<String>) -> Self {
        self.alt = alt;
        self
    }

    /// Depth-first walk (self first), depth-capped so pathological trees
    /// cannot exhaust the stack.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Node)) {
        walk_node(self, f, 0);
    }
}

fn walk_node<'a>(node: &'a Node, f: &mut dyn FnMut(&'a Node), depth: usize) {
    const MAX_DEPTH: usize = 128;
    if depth > MAX_DEPTH {
        return;
    }
    f(node);
    for c in &node.children {
        walk_node(c, f, depth + 1);
    }
}
