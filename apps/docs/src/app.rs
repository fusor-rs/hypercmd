use docs_base::{Location, Navigation, Site};
use fusor::{
    OwnerHandle,
    dom::{Content, FromInputs, JsValue},
};
use fusor_router::browser::declarative::Navigation as RouterNavigation;

include!(concat!(env!("OUT_DIR"), "/guides.rs"));

static SITE: Site = Site {
    name: "hypercmd",
    base_path: "/docs/",
    logo: "/docs/favicon.svg",
    version: "v0.1 dev",
    pages: PAGES,
};

struct App {
    navigation: Navigation,
    header: Content,
}

impl App {
    fn new() -> Self {
        Self {
            navigation: Navigation::new(&SITE),
            header: Content::new(|_| Header),
        }
    }
}

struct Header;

struct DocRoute {
    index: Option<usize>,
}

struct DocRouteInputs {
    navigation: Navigation,
}

impl FromInputs for DocRoute {
    type Inputs = DocRouteInputs;
    type Error = JsValue;

    fn from_inputs(inputs: Self::Inputs, owner: OwnerHandle) -> Result<Self, JsValue> {
        let router = RouterNavigation::from_owner(&owner)
            .ok_or_else(|| JsValue::from_str("missing documentation navigation"))?;
        let index = match router.location().get().segments() {
            Ok(segments) => {
                let slug = segments.join("/");
                PAGES.iter().position(|page| page.slug == slug)
            }
            // Malformed route encodings use the same missing-page view as unknown paths.
            Err(_) => None,
        };
        inputs
            .navigation
            .active
            .set(index.map_or(Location::Missing, Location::Guide));
        Ok(Self { index })
    }
}

#[expect(
    clippy::excessive_nesting,
    reason = "generated DOM scaffolding: https://github.com/fusor-rs/fusor/issues/17"
)]
mod dom {
    use super::{App, DocRoute, Header, SITE};
    use docs_base::{Article, ArticleExtras, Shell};
    fusor::template!("web/index.html");
}
