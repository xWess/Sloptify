use dioxus::prelude::*;
use tw_merge::tw_merge;

#[derive(Default, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
pub enum SliderVariant {
    #[default]
    Round,
    Flat,
}

impl SliderVariant {
    fn as_str(self) -> &'static str {
        self.into()
    }
}

#[component]
pub fn Slider(
    #[props(into, optional)] class: Option<String>,
    #[props(default)] variant: SliderVariant,
    #[props(default = 0.0)] min: f64,
    #[props(default = 100.0)] max: f64,
    #[props(default = 1.0)] step: f64,
    #[props(default = 50.0)] value: f64,
    /// When false, do not bind `value` so Chromium keeps pointer capture while scrubbing.
    /// Re-writing the value attribute on every input event cancels the drag gesture.
    #[props(default = true)]
    bind_value: bool,
    #[props(optional)] disabled: bool,
    #[props(optional)] oninput: Option<EventHandler<FormEvent>>,
    #[props(optional)] onchange: Option<EventHandler<FormEvent>>,
) -> Element {
    // Do NOT use overflow-hidden — it clips the thumb and kills drag in WebKit.
    let merged = tw_merge!(
        "relative bg-transparent appearance-none disabled:opacity-30 disabled:cursor-not-allowed w-full max-w-none cursor-pointer",
        class.as_deref().unwrap_or("")
    );
    let value = value.clamp(min, max);
    let bound = bind_value.then(|| format!("{value}"));

    rsx! {
        input {
            "data-name": "Slider",
            "data-variant": "{variant.as_str()}",
            r#type: "range",
            class: "{merged}",
            min: "{min}",
            max: "{max}",
            step: "{step}",
            value: bound,
            disabled: disabled,
            oninput: move |e| {
                if let Some(handler) = &oninput {
                    handler.call(e);
                }
            },
            onchange: move |e| {
                if let Some(handler) = &onchange {
                    handler.call(e);
                }
            },
        }
    }
}
