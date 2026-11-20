//! Labelled form controls bound to signals.

use leptos::prelude::*;

/// A single-line text (or password) input with a label.
#[component]
pub fn TextField(
    /// Label text.
    label: &'static str,
    /// Signal holding the current value.
    value: RwSignal<String>,
    /// Placeholder text.
    #[prop(optional)]
    placeholder: &'static str,
    /// HTML input type, `text` by default.
    #[prop(default = "text")]
    input_type: &'static str,
) -> impl IntoView {
    view! {
        <label>
            {label}
            <input
                type=input_type
                placeholder=placeholder
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
        </label>
    }
}

/// A multi-line input with a label.
#[component]
pub fn TextAreaField(
    /// Label text.
    label: &'static str,
    /// Signal holding the current value.
    value: RwSignal<String>,
    /// Placeholder text.
    #[prop(optional)]
    placeholder: &'static str,
) -> impl IntoView {
    view! {
        <label>
            {label}
            <textarea
                rows="3"
                placeholder=placeholder
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            ></textarea>
        </label>
    }
}

/// A checkbox with a label.
#[component]
pub fn CheckboxField(
    /// Label text.
    label: &'static str,
    /// Signal holding the checked state.
    value: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <label class="check">
            <input
                type="checkbox"
                prop:checked=move || value.get()
                on:change=move |ev| value.set(event_target_checked(&ev))
            />
            {label}
        </label>
    }
}

/// A drop-down with a label.
#[component]
pub fn SelectField(
    /// Label text.
    label: &'static str,
    /// Signal holding the selected option value.
    value: RwSignal<String>,
    /// The available option values.
    options: &'static [&'static str],
) -> impl IntoView {
    view! {
        <label>
            {label}
            <select on:change=move |ev| value.set(event_target_value(&ev))>
                {options
                    .iter()
                    .map(|option| {
                        let option = *option;
                        view! {
                            <option value=option selected=move || value.get() == option>
                                {option}
                            </option>
                        }
                    })
                    .collect_view()}
            </select>
        </label>
    }
}
