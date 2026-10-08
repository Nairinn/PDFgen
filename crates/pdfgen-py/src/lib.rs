//! Python bindings for pdfgen (PyO3).
//!
//! ```python
//! import pdfgen
//! doc = pdfgen.Document(pdfgen.Profile.UA1, title="Report", lang="en-US")
//! doc.heading(1, "Quarterly Report")
//! doc.paragraph("Revenue is up.")
//! report = doc.save("report.pdf")
//! if not report.compliant:
//!     print(report.note)
//! ```

use pdfgen_api::{PdfDocument, TargetProfile};
use pyo3::create_exception;
use pyo3::exceptions::PyWarning;
use pyo3::prelude::*;
use std::collections::HashSet;

create_exception!(pdfgen, PdfError, pyo3::exceptions::PyValueError);
create_exception!(pdfgen, PdfUaWarning, PyWarning);

/// Conformance profile.
#[derive(Clone, Copy, PartialEq)]
#[pyclass(eq, eq_int)]
pub enum Profile {
    /// PDF/UA-1 (PDF 1.7).
    UA1 = 1,
    /// PDF/UA-2 (PDF 2.0).
    UA2 = 2,
}

impl From<Profile> for TargetProfile {
    fn from(p: Profile) -> Self {
        match p {
            Profile::UA1 => TargetProfile::Ua1,
            Profile::UA2 => TargetProfile::Ua2,
        }
    }
}

/// The save report returned by `Document.save()`.
#[pyclass]
struct Report {
    inner: pdfgen_api::Report,
}

#[pymethods]
impl Report {
    /// Profile name, e.g. "PDF/UA-1".
    #[getter]
    fn profile_name(&self) -> String {
        self.inner.profile_name.clone()
    }

    /// True when all machine checks passed and the file claims PDF/UA.
    #[getter]
    fn compliant(&self) -> bool {
        self.inner.compliant
    }

    /// Violations as a list of (id, message, fix) tuples.
    #[getter]
    fn violations(&self) -> Vec<(String, String, String)> {
        self.inner.violations.clone()
    }

    /// Human-review checklist items.
    #[getter]
    fn human_review(&self) -> Vec<String> {
        self.inner.human_review.clone()
    }

    /// The user-facing note; empty when compliant.
    #[getter]
    fn note(&self) -> String {
        self.inner.note.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "<pdfgen.Report {} compliant={} violations={}>",
            self.inner.profile_name,
            self.inner.compliant,
            self.inner.violations.len()
        )
    }
}

/// A PDF/UA document under construction.
#[pyclass]
struct Document {
    doc: PdfDocument,
    /// Notes already warned about (dedupe repeated saves).
    warned: HashSet<String>,
}

#[pymethods]
impl Document {
    #[new]
    #[pyo3(signature = (profile, *, title=None, lang=None))]
    fn new(profile: Profile, title: Option<String>, lang: Option<String>) -> Self {
        let doc = PdfDocument::new(profile.into());
        if let Some(t) = &title {
            doc.set_title(t);
        }
        if let Some(l) = &lang {
            doc.set_lang(l);
        }
        Document {
            doc,
            warned: HashSet::new(),
        }
    }

    /// Set the document title.
    fn set_title(&self, title: String) {
        self.doc.set_title(&title);
    }

    /// Set the primary natural language, e.g. "en-US".
    fn set_lang(&self, lang: String) {
        self.doc.set_lang(&lang);
    }

    /// Resolve a font by family and style ("Regular", "Bold", "Italic",
    /// "Bold Italic"). Returns a font handle.
    fn font(&self, family: String, style: String) -> PyResult<u32> {
        self.doc.font(&family, &style).map_err(PdfError::new_err)
    }

    /// Load a font file directly by path.
    fn load_font_file(&self, path: String) -> PyResult<u32> {
        self.doc.load_font_file(&path).map_err(PdfError::new_err)
    }

    /// Register a font file under a family name for later font() calls.
    fn register_font(&self, family: String, style: String, path: String) {
        self.doc.register_font(&family, &style, &path);
    }

    /// Add a heading (level 1-6).
    fn heading(&self, level: u8, text: String) -> PyResult<()> {
        self.doc.heading(level, &text).map_err(PdfError::new_err)
    }

    /// Add a paragraph in the default font.
    fn paragraph(&self, text: String) -> PyResult<()> {
        self.doc.paragraph(&text).map_err(PdfError::new_err)
    }

    /// Add a paragraph in a specific font handle and size.
    fn paragraph_in(&self, font: u32, size: f64, text: String) -> PyResult<()> {
        self.doc
            .paragraph_in(font, size, &text)
            .map_err(PdfError::new_err)
    }

    /// Add a bullet list.
    fn bullet_list(&self, items: Vec<String>) -> PyResult<()> {
        self.doc.bullet_list(items).map_err(PdfError::new_err)
    }

    /// Add a table. `widths` may be empty for equal columns.
    fn table(&self, header: Vec<String>, rows: Vec<Vec<String>>, widths: Vec<f64>) -> PyResult<()> {
        self.doc
            .table(header, rows, widths)
            .map_err(PdfError::new_err)
    }

    /// Place an image file (PNG/JPEG) as a figure with alt text. Empty alt
    /// marks the image decorative.
    fn figure(&self, path: String, alt: String, width: f64, height: f64) -> PyResult<()> {
        self.doc
            .figure(&path, &alt, width, height)
            .map_err(PdfError::new_err)
    }

    /// Page header text (pagination artifact).
    fn page_header(&self, text: String) -> PyResult<()> {
        self.doc.page_header(&text).map_err(PdfError::new_err)
    }

    /// Page footer text (pagination artifact).
    fn page_footer(&self, text: String) -> PyResult<()> {
        self.doc.page_footer(&text).map_err(PdfError::new_err)
    }

    /// Save the file (ALWAYS writes it) and return the accessibility
    /// report. Emits a PdfUaWarning when not compliant yet.
    fn save(&mut self, py: Python<'_>, path: String) -> PyResult<Py<Report>> {
        let report = self.doc.save(&path).map_err(PdfError::new_err)?;
        if !report.compliant && self.warned.insert(report.note.clone()) {
            use pyo3::PyTypeInfo;
            use std::ffi::CString;
            let note = CString::new(report.note.as_str())
                .map_err(|_| PdfError::new_err("note contains an interior NUL byte"))?;
            let ty = PdfUaWarning::type_object(py);
            let _ = PyErr::warn(py, ty.as_any(), note.as_c_str(), 1);
        }
        let r = Report { inner: report };
        Py::new(py, r)
    }

    /// Context-manager support.
    fn __enter__(slf: Py<Self>, _py: Python<'_>) -> Py<Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exc_type: PyObject,
        _exc_value: PyObject,
        _tb: PyObject,
    ) -> PyResult<bool> {
        // Swallow nothing; the document just goes out of scope.
        Ok(false)
    }
}

/// Module init: expose the classes and exceptions. The pymodule name MUST
/// match the cdylib name (pdfgen) so maturin finds the init symbol.
#[pymodule]
fn pdfgen(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Profile>()?;
    m.add_class::<Document>()?;
    m.add_class::<Report>()?;
    m.add("PdfError", m.py().get_type::<PdfError>())?;
    m.add("PdfUaWarning", m.py().get_type::<PdfUaWarning>())?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
