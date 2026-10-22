//! Typed loader for the mtg-bot **card IR** (see `docs/card_ir.md`).
//!
//! mtg-bot's oracle parser exports every parsed card as JSON (`$t` class
//! tags, sparse fields). The types in [`generated`] are produced from the
//! exported schema by `tools/codegen/gen_card_ir.py`; this module adds the
//! document wrapper and a loader that checks the format, IR version and
//! schema hash.

pub mod generated;
pub mod support;

pub use generated::{cats, enums, nodes, unions, IR_VERSION, SCHEMA_SHA256, TAG_CATEGORY};
pub use support::{Count, Never};

use serde::Deserialize;

/// The `format` value of an IR document.
pub const IR_FORMAT: &str = "mtg-bot-card-ir";

/// One encoded card: Scryfall `oracle_id` (absent only for custom cards),
/// name, and the parsed printing.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub oracle_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub custom: bool,
    pub printing: cats::CardPrinting,
}

/// A whole IR document (one card pool).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub format: String,
    pub ir_version: u32,
    pub pool: String,
    pub schema_sha256: String,
    pub cards: Vec<Card>,
}

#[derive(Debug)]
pub enum LoadError {
    Json(serde_json::Error),
    Format(String),
    Version(u32),
    Schema { document: String, generated: &'static str },
    Card { name: String, error: serde_json::Error },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Json(e) => write!(f, "invalid IR JSON: {e}"),
            LoadError::Format(s) => write!(f, "not a card IR document (format {s:?})"),
            LoadError::Version(v) => {
                write!(f, "IR version {v}; this crate reads version {IR_VERSION}")
            }
            LoadError::Schema { document, generated } => write!(
                f,
                "document schema {document} differs from the generated types' schema {generated}; \
                 regenerate card_ir/src/generated.rs"
            ),
            LoadError::Card { name, error } => write!(f, "card {name:?}: {error}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Parse an IR document, checking its header against the generated types.
/// Cards are decoded one by one so an error names the card.
pub fn load_document(json: &str) -> Result<Document, LoadError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        format: String,
        ir_version: u32,
        pool: String,
        schema_sha256: String,
        cards: Vec<serde_json::Value>,
    }
    let raw: Raw = serde_json::from_str(json).map_err(LoadError::Json)?;
    if raw.format != IR_FORMAT {
        return Err(LoadError::Format(raw.format));
    }
    if raw.ir_version != IR_VERSION {
        return Err(LoadError::Version(raw.ir_version));
    }
    if raw.schema_sha256 != SCHEMA_SHA256 {
        return Err(LoadError::Schema { document: raw.schema_sha256, generated: SCHEMA_SHA256 });
    }
    let mut cards = Vec::with_capacity(raw.cards.len());
    for v in raw.cards {
        let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
        let card: Card =
            serde_json::from_value(v).map_err(|error| LoadError::Card { name, error })?;
        cards.push(card);
    }
    Ok(Document {
        format: raw.format,
        ir_version: raw.ir_version,
        pool: raw.pool,
        schema_sha256: raw.schema_sha256,
        cards,
    })
}

/// The category of a `$t` tag, if the schema knows it.
pub fn category_of(tag: &str) -> Option<&'static str> {
    TAG_CATEGORY
        .binary_search_by(|(t, _)| (*t).cmp(tag))
        .ok()
        .map(|i| TAG_CATEGORY[i].1)
}
