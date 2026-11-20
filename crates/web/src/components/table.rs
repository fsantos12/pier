//! Message banners shown above tables and forms.

use leptos::prelude::*;

/// Visual style of a [`Banner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerKind {
    /// A failed action.
    Error,
    /// A successful action.
    Ok,
}

/// Shows the message in `message` (if any) as a coloured banner.
#[component]
pub fn Banner(
    /// The message to show; `None` hides the banner.
    #[prop(into)]
    message: Signal<Option<String>>,
    /// Banner style.
    kind: BannerKind,
) -> impl IntoView {
    let class = match kind {
        BannerKind::Error => "banner error",
        BannerKind::Ok => "banner ok",
    };
    view! {
        {move || message.get().map(|text| view! { <div class=class role="status">{text}</div> })}
    }
}
