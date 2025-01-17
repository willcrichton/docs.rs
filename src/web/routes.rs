use super::{
    cache::CachePolicy, error::AxumNope, metrics::request_recorder, metrics::RequestRecorder,
};
use axum::{
    handler::Handler as AxumHandler,
    http::Request as AxumHttpRequest,
    middleware::{self, Next},
    response::{IntoResponse, Redirect},
    routing::get,
    routing::MethodRouter,
    Router as AxumRouter,
};
use axum_extra::routing::RouterExt;
use iron::middleware::Handler;
use router::Router as IronRouter;
use std::{collections::HashSet, convert::Infallible};
use tracing::{debug, instrument};

const INTERNAL_PREFIXES: &[&str] = &["-", "about", "crate", "releases", "sitemap.xml"];

#[instrument(skip_all)]
fn get_static<H, T, S, B>(handler: H) -> MethodRouter<S, B, Infallible>
where
    H: AxumHandler<T, S, B>,
    B: Send + 'static + hyper::body::HttpBody,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    get(handler).route_layer(middleware::from_fn(|request, next| async {
        request_recorder(request, next, Some("static resource")).await
    }))
}

#[instrument(skip_all)]
fn get_internal<H, T, S, B>(handler: H) -> MethodRouter<S, B, Infallible>
where
    H: AxumHandler<T, S, B>,
    B: Send + 'static + hyper::body::HttpBody,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    get(handler).route_layer(middleware::from_fn(|request, next| async {
        request_recorder(request, next, None).await
    }))
}

#[instrument(skip_all)]
fn get_rustdoc<H, T, S, B>(handler: H) -> MethodRouter<S, B, Infallible>
where
    H: AxumHandler<T, S, B>,
    B: Send + 'static + hyper::body::HttpBody,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    get(handler)
        .route_layer(middleware::from_fn(|request, next| async {
            request_recorder(request, next, Some("rustdoc page")).await
        }))
        .layer(middleware::from_fn(block_blacklisted_prefixes_middleware))
}

#[instrument(skip_all)]
async fn block_blacklisted_prefixes_middleware<B>(
    request: AxumHttpRequest<B>,
    next: Next<B>,
) -> impl IntoResponse {
    if let Some(first_component) = request.uri().path().trim_matches('/').split('/').next() {
        if !first_component.is_empty()
            && (INTERNAL_PREFIXES.binary_search(&first_component).is_ok())
        {
            debug!(
                first_component = first_component,
                "blocking blacklisted prefix"
            );
            return AxumNope::CrateNotFound.into_response();
        }
    }

    next.run(request).await
}

pub(super) fn build_axum_routes() -> AxumRouter {
    AxumRouter::new()
        // Well known resources, robots.txt and favicon.ico support redirection, the sitemap.xml
        // must live at the site root:
        //   https://developers.google.com/search/reference/robots_txt#handling-http-result-codes
        //   https://support.google.com/webmasters/answer/183668?hl=en
        .route(
            "/robots.txt",
            get_static(|| async { Redirect::permanent("/-/static/robots.txt") }),
        )
        .route(
            "/favicon.ico",
            get_static(|| async { Redirect::permanent("/-/static/favicon.ico") }),
        )
        .route(
            "/-/static/*path",
            get_static(super::statics::static_handler),
        )
        .route(
            "/opensearch.xml",
            get_static(|| async { Redirect::permanent("/-/static/opensearch.xml") }),
        )
        .route_with_tsr(
            "/sitemap.xml",
            get_internal(super::sitemap::sitemapindex_handler),
        )
        .route_with_tsr(
            "/-/sitemap/:letter/sitemap.xml",
            get_internal(super::sitemap::sitemap_handler),
        )
        .route_with_tsr(
            "/about/builds",
            get_internal(super::sitemap::about_builds_handler),
        )
        .route_with_tsr(
            "/about/metrics",
            get_internal(super::metrics::metrics_handler),
        )
        .route_with_tsr("/about", get_internal(super::sitemap::about_handler))
        .route_with_tsr(
            "/about/:subpage",
            get_internal(super::sitemap::about_handler),
        )
        .route("/", get_internal(super::releases::home_page))
        .route_with_tsr(
            "/releases",
            get_internal(super::releases::recent_releases_handler),
        )
        .route_with_tsr(
            "/releases/recent/:page",
            get_internal(super::releases::recent_releases_handler),
        )
        .route_with_tsr(
            "/releases/stars",
            get_internal(super::releases::releases_by_stars_handler),
        )
        .route_with_tsr(
            "/releases/stars/:page",
            get_internal(super::releases::releases_by_stars_handler),
        )
        .route_with_tsr(
            "/releases/recent-failures",
            get_internal(super::releases::releases_recent_failures_handler),
        )
        .route_with_tsr(
            "/releases/recent-failures/:page",
            get_internal(super::releases::releases_recent_failures_handler),
        )
        .route_with_tsr(
            "/releases/failures",
            get_internal(super::releases::releases_failures_by_stars_handler),
        )
        .route_with_tsr(
            "/releases/failures/:page",
            get_internal(super::releases::releases_failures_by_stars_handler),
        )
        .route_with_tsr(
            "/crate/:name",
            get_internal(super::crate_details::crate_details_handler),
        )
        .route_with_tsr(
            "/crate/:name/:version",
            get_internal(super::crate_details::crate_details_handler),
        )
        .route_with_tsr(
            "/releases/feed",
            get_static(super::releases::releases_feed_handler),
        )
        .route_with_tsr(
            "/releases/:owner",
            get_internal(super::releases::owner_handler),
        )
        .route_with_tsr(
            "/releases/:owner/:page",
            get_internal(super::releases::owner_handler),
        )
        .route_with_tsr(
            "/releases/activity",
            get_internal(super::releases::activity_handler),
        )
        .route_with_tsr(
            "/releases/search",
            get_internal(super::releases::search_handler),
        )
        .route_with_tsr(
            "/releases/queue",
            get_internal(super::releases::build_queue_handler),
        )
        .route_with_tsr(
            "/crate/:name/:version/builds",
            get_internal(super::builds::build_list_handler),
        )
        .route(
            "/crate/:name/:version/builds.json",
            get_static(super::builds::build_list_json_handler),
        )
        .route_with_tsr(
            "/crate/:name/:version/builds/:id",
            get_internal(super::build_details::build_details_handler),
        )
        .route_with_tsr(
            "/crate/:name/:version/features",
            get_internal(super::features::build_features_handler),
        )
        .route_with_tsr(
            "/crate/:name/:version/source/",
            get_internal(super::source::source_browser_handler),
        )
        .route(
            "/crate/:name/:version/source/*path",
            get_internal(super::source::source_browser_handler),
        )
        .route(
            "/-/rustdoc.static/*path",
            get_internal(super::rustdoc::static_asset_handler),
        )
        .route(
            "/-/storage-change-detection.html",
            get_internal(|| async {
                #[derive(Debug, Clone, serde::Serialize)]
                struct StorageChangeDetection {}
                crate::impl_axum_webpage!(
                    StorageChangeDetection = "storage-change-detection.html",
                    cache_policy = |_| CachePolicy::ForeverInCdnAndBrowser,
                );
                StorageChangeDetection {}
            }),
        )
        .route_with_tsr(
            "/crate/:name/:version/download",
            get_internal(super::rustdoc::download_handler),
        )
        .route(
            "/crate/:name/:version/target-redirect/*path",
            get_internal(super::rustdoc::target_redirect_handler),
        )
        .route(
            "/:crate/badge.svg",
            get_rustdoc(super::rustdoc::badge_handler),
        )
}

// REFACTOR: Break this into smaller initialization functions
pub(super) fn build_routes() -> Routes {
    let mut routes = Routes::new();

    routes.rustdoc_page("/:crate", super::rustdoc::rustdoc_redirector_handler);
    routes.rustdoc_page("/:crate/", super::rustdoc::rustdoc_redirector_handler);
    routes.rustdoc_page(
        "/:crate/:version",
        super::rustdoc::rustdoc_redirector_handler,
    );
    routes.rustdoc_page(
        "/:crate/:version/",
        super::rustdoc::rustdoc_redirector_handler,
    );
    routes.rustdoc_page(
        "/:crate/:version/settings.html",
        super::rustdoc::rustdoc_html_server_handler,
    );
    routes.rustdoc_page(
        "/:crate/:version/scrape-examples-help.html",
        super::rustdoc::rustdoc_html_server_handler,
    );
    routes.rustdoc_page(
        "/:crate/:version/all.html",
        super::rustdoc::rustdoc_html_server_handler,
    );
    routes.rustdoc_page(
        "/:crate/:version/:target",
        super::rustdoc::rustdoc_redirector_handler,
    );
    routes.rustdoc_page(
        "/:crate/:version/:target/",
        super::rustdoc::rustdoc_html_server_handler,
    );
    routes.rustdoc_page(
        "/:crate/:version/:target/*.html",
        super::rustdoc::rustdoc_html_server_handler,
    );

    for prefix in INTERNAL_PREFIXES {
        routes.add_internal_page_prefix(prefix);
    }

    routes
}

/// This wrapper class aids the construction of iron's Router, with docs.rs-specific additions to
/// it. Routes are supposed to be added by the build_routes function, which calls methods in this
/// struct depending on the type of route being added.
pub(super) struct Routes {
    /// Normal GET routes.
    get: Vec<(String, Box<dyn Handler>)>,
    /// GET routes serving rustdoc content. The BlockBlacklistedPrefixes middleware is added
    /// automatically to all of them.
    rustdoc_get: Vec<(String, Box<dyn Handler>)>,
    /// Prefixes of all the internal routes. This data is used to power the
    /// BlockBlacklistedPrefixes middleware.
    page_prefixes: HashSet<String>,
}

impl Routes {
    fn new() -> Self {
        Self {
            get: Vec::new(),
            rustdoc_get: Vec::new(),
            page_prefixes: HashSet::new(),
        }
    }

    pub(super) fn page_prefixes(&self) -> HashSet<String> {
        self.page_prefixes.clone()
    }

    pub(super) fn add_internal_page_prefix<P: AsRef<str>>(&mut self, prefix: P) {
        self.page_prefixes.insert(prefix.as_ref().to_string());
    }

    pub(super) fn iron_router(mut self) -> IronRouter {
        let mut router = IronRouter::new();
        for (pattern, handler) in self.get.drain(..) {
            router.get(&pattern, handler, calculate_id(&pattern));
        }

        // All rustdoc pages have the prefixes of other docs.rs pages blacklisted. This prevents,
        // for example, a crate named "about" from hijacking /about/0.1.0/index.html.
        let blacklist = self.page_prefixes();
        for (pattern, handler) in self.rustdoc_get.drain(..) {
            router.get(
                &pattern,
                BlockBlacklistedPrefixes::new(blacklist.clone(), handler),
                calculate_id(&pattern),
            );
        }

        router
    }

    /// A rustdoc page is a page serving generated documentation. It's similar to a static
    /// resource, but path prefixes are automatically blacklisted (see internal pages to learn more
    /// about page prefixes).
    fn rustdoc_page(&mut self, pattern: &str, handler: impl Handler) {
        self.get.push((
            pattern.to_string(),
            Box::new(RequestRecorder::new(handler, "rustdoc page")),
        ));
    }
}

#[derive(Copy, Clone)]
struct PermanentRedirect(&'static str);

impl Handler for PermanentRedirect {
    fn handle(&self, _req: &mut iron::Request) -> iron::IronResult<iron::Response> {
        Ok(iron::Response::with((
            iron::status::MovedPermanently,
            iron::modifiers::RedirectRaw(self.0.to_owned()),
        )))
    }
}

/// Iron Middleware that prevents requests to blacklisted prefixes.
///
/// In our application, a prefix is blacklisted if a docs.rs page exists below it. For example,
/// since /releases/queue is a docs.rs page, /releases is a blacklisted prefix.
///
/// The middleware must be used for all the pages serving crates at the top level, to prevent a
/// crate from putting their own content in an URL that's supposed to be used by docs.rs.
pub(super) struct BlockBlacklistedPrefixes {
    blacklist: HashSet<String>,
    handler: Box<dyn Handler>,
}

impl BlockBlacklistedPrefixes {
    pub(super) fn new(blacklist: HashSet<String>, handler: Box<dyn Handler>) -> Self {
        Self { blacklist, handler }
    }
}

impl Handler for BlockBlacklistedPrefixes {
    fn handle(&self, req: &mut iron::Request) -> iron::IronResult<iron::Response> {
        if let Some(prefix) = req.url.path().first() {
            if self.blacklist.contains(*prefix) {
                return Err(super::error::Nope::CrateNotFound.into());
            }
        }
        self.handler.handle(req)
    }
}

/// Automatically generate a Route ID from a pattern. Every non-alphanumeric character is replaced
/// with `_`.
fn calculate_id(pattern: &str) -> String {
    let calculate_char = |c: char| {
        if c.is_alphanumeric() || c == '-' {
            c
        } else {
            '_'
        }
    };

    pattern.chars().map(calculate_char).collect()
}

#[cfg(test)]
mod tests {
    use crate::test::*;
    use crate::web::cache::CachePolicy;
    use reqwest::StatusCode;

    #[test]
    fn test_root_redirects() {
        wrapper(|env| {
            // These are "well-known" resources that will be requested from the root, but support
            // redirection
            assert_redirect("/favicon.ico", "/-/static/favicon.ico", env.frontend())?;
            assert_redirect("/robots.txt", "/-/static/robots.txt", env.frontend())?;

            // This has previously been served with a url pointing to the root, it may be
            // plausible to remove the redirects in the future, but for now we need to keep serving
            // it.
            assert_redirect(
                "/opensearch.xml",
                "/-/static/opensearch.xml",
                env.frontend(),
            )?;

            Ok(())
        });
    }

    #[test]
    fn serve_rustdoc_content_not_found() {
        wrapper(|env| {
            let response = env.frontend().get("/-/rustdoc.static/style.css").send()?;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_cache_control(&response, CachePolicy::NoCaching, &env.config());
            Ok(())
        })
    }

    #[test]
    fn serve_rustdoc_content() {
        wrapper(|env| {
            let web = env.frontend();
            env.storage()
                .store_one("/rustdoc-static/style.css", "content".as_bytes())?;
            env.storage()
                .store_one("/will_not/be_found.css", "something".as_bytes())?;

            let response = web.get("/-/rustdoc.static/style.css").send()?;
            assert!(response.status().is_success());
            assert_cache_control(
                &response,
                CachePolicy::ForeverInCdnAndBrowser,
                &env.config(),
            );
            assert_eq!(response.text()?, "content");

            assert_eq!(
                web.get("/-/rustdoc.static/will_not/be_found.css")
                    .send()?
                    .status(),
                StatusCode::NOT_FOUND
            );
            Ok(())
        })
    }
}
