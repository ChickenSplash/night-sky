//! NIGHT SKY PATCH: opt-in, pre-navigation gate for the embedded experiment.
use crate::prelude::IntoString;
use cef::*;

fn allowed(url: &str) -> bool {
    url.strip_prefix("cef://localhost/__inline__/")
        .is_some_and(|id| {
            !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

wrap_request_handler! {
    struct InlineRequestHandler;
    impl RequestHandler {
        fn on_before_browse(
            &self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>,
            request: Option<&mut Request>, _user_gesture: i32, _is_redirect: i32,
        ) -> i32 {
            i32::from(!request.is_some_and(|r| allowed(&r.url().into_string())))
        }
        fn on_open_urlfrom_tab(
            &self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>,
            _target_url: Option<&CefString>, _target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
        ) -> i32 { 1 }
    }
}

wrap_life_span_handler! {
    struct InlineLifeSpanHandler;
    impl LifeSpanHandler {
        fn on_before_popup(
            &self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _popup_id: i32,
            _target_url: Option<&CefString>, _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition, _user_gesture: i32,
            _popup_features: Option<&PopupFeatures>, _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>, _no_javascript_access: Option<&mut i32>,
        ) -> i32 { 1 }
    }
}

pub fn request_handler() -> RequestHandler {
    InlineRequestHandler::new()
}
pub fn life_span_handler() -> LifeSpanHandler {
    InlineLifeSpanHandler::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_inline_documents_are_allowed() {
        assert!(allowed("cef://localhost/__inline__/123-abc"));
        for url in [
            "https://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "cef://localhost/other.html",
            "cef://localhost/__inline__/../other",
            "about:blank",
        ] {
            assert!(!allowed(url), "{url}");
        }
    }
}
