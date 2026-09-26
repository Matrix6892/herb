//! Page templates (`site/templates/`) and the view models they render. View
//! models hold display-ready strings; templates hold markup and keys.

use askama::Template;

use crate::i18n::Tr;

pub struct Layout {
    pub lang: String,
    pub dir: String,
    pub title: String,
    pub description: String,
    pub canonical: String,
    pub noindex: bool,
    /// The whole stylesheet, inlined so first paint needs no request.
    pub css: String,
    pub js: String,
    pub api: String,
    pub updated: Option<String>,
    pub nav: Vec<(String, String)>,
}

#[derive(Template)]
#[template(path = "buy_button.html")]
pub struct BuyButton<'a> {
    pub t: &'a Tr,
    pub id: String,
    pub href: Option<String>,
}

pub struct TabView {
    pub key: &'static str,
    pub label: String,
    pub note: String,
    pub href: String,
    pub current: bool,
    /// Why the preset ranks nothing; `None` when it ranks something.
    pub empty: Option<String>,
}

pub struct CardView {
    pub id: String,
    /// Position in the default preset; zero when not in it.
    pub rank: usize,
    /// In another preset only; shown when JavaScript switches to it.
    pub hidden: bool,
    pub href: String,
    pub title: String,
    pub brand: String,
    pub unit_price: String,
    pub per_unit: String,
    pub container: String,
    pub rating: Option<String>,
    pub why: Vec<String>,
    pub out_of_stock: bool,
    pub buy: String,
}

pub struct UnrankedView {
    pub href: String,
    pub name: String,
    pub reason: String,
}

#[derive(Template)]
#[template(path = "category.html")]
pub struct CategoryPage<'a> {
    pub t: &'a Tr,
    pub l: Layout,
    pub title: String,
    pub lede: String,
    pub counts: String,
    pub tabs: Vec<TabView>,
    pub default_key: &'static str,
    /// Preset key and the space-separated ids in its order.
    pub orders: Vec<(&'static str, String)>,
    /// In the default preset's order.
    pub cards: Vec<CardView>,
    pub unranked: Vec<UnrankedView>,
}

pub struct PriceView {
    pub unit_price: String,
    pub per_unit: String,
    pub container: String,
    pub formula: String,
    pub date: String,
}

pub struct LineView {
    pub n: usize,
    pub printed: String,
    pub counted_as: String,
    pub factor: String,
    pub result: String,
}

pub struct LabelView {
    pub result_heading: String,
    pub lines: Vec<LineView>,
    pub per_serving: Option<String>,
    pub verified: String,
    pub formula_changed: Option<String>,
    pub third_party: String,
    pub sources: Vec<(String, String)>,
}

pub struct NeighbourView {
    pub rank: usize,
    pub href: String,
    pub name: String,
    pub unit_price: String,
    pub is_self: bool,
}

#[derive(Template)]
#[template(path = "product.html")]
pub struct ProductPage<'a> {
    pub t: &'a Tr,
    pub l: Layout,
    pub id: String,
    pub path: String,
    pub crumb_href: String,
    pub crumb_name: String,
    pub name: String,
    pub brand: String,
    pub price: Option<PriceView>,
    pub not_compared: Option<String>,
    pub out_of_stock: bool,
    pub buy: String,
    pub positions: Vec<String>,
    pub label: Option<LabelView>,
    pub neighbours_title: String,
    pub neighbours: Vec<NeighbourView>,
    pub show_rating_field: bool,
}

#[derive(Template)]
#[template(path = "product_pending.html")]
pub struct PendingPage<'a> {
    pub t: &'a Tr,
    pub l: Layout,
    pub crumb_href: String,
    pub crumb_name: String,
    pub heading: String,
    pub brand: String,
    pub queued: Option<String>,
    pub buy: String,
}

pub struct ControlRowView {
    pub name: String,
    pub p_unit: String,
    pub r_b: String,
    pub q_price: String,
    pub q_rating: String,
    pub s: String,
}

pub struct ControlView {
    pub rows: Vec<ControlRowView>,
    pub order: String,
}

#[derive(Template)]
#[template(path = "how.html")]
pub struct HowPage<'a> {
    pub t: &'a Tr,
    pub l: Layout,
    pub dose_example: Option<String>,
    pub unit_text: String,
    pub unit_example: Option<String>,
    /// Shown in rating branch A only.
    pub control: Option<ControlView>,
    pub presets: Vec<(String, String)>,
    pub ratings_shown: bool,
}

pub struct HomeCategory {
    pub href: String,
    pub name: String,
    pub counts: String,
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexPage<'a> {
    pub t: &'a Tr,
    pub l: Layout,
    pub categories: Vec<HomeCategory>,
}

#[derive(Template)]
#[template(path = "disclosure.html")]
pub struct DisclosurePage<'a> {
    pub t: &'a Tr,
    pub l: Layout,
}

#[derive(Template)]
#[template(path = "privacy.html")]
pub struct PrivacyPage<'a> {
    pub t: &'a Tr,
    pub l: Layout,
}

#[derive(Template)]
#[template(path = "not_found.html")]
pub struct NotFoundPage<'a> {
    pub t: &'a Tr,
    pub l: Layout,
}
