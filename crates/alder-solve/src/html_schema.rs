//! HTML Living Standard schema. Generated vocabulary plus explicit Alder type
//! policy for events, custom elements and ARIA. See tools/generate-html-schema.py.
mod generated;

#[derive(Clone, Copy)]
pub(crate) enum AttributeType {
    String,
    Number,
    Bool,
}

pub(crate) struct Element {
    pub name: &'static str,
    pub phrasing: bool,
    pub phrasing_children: bool,
    pub void: bool,
    attrs: &'static [(&'static str, AttributeType)],
}

pub(crate) fn element(name: &str) -> Option<&'static Element> {
    generated::ELEMENTS
        .iter()
        .find(|element| element.name == name)
}

pub(crate) fn custom(name: &str) -> bool {
    name.contains('-')
        && !matches!(
            name,
            "annotation-xml"
                | "color-profile"
                | "font-face"
                | "font-face-src"
                | "font-face-uri"
                | "font-face-format"
                | "font-face-name"
                | "missing-glyph"
        )
}

pub(crate) fn attribute(tag: &str, name: &str) -> Option<AttributeType> {
    if name
        .strip_prefix("data-")
        .is_some_and(|suffix| !suffix.is_empty())
        || name == "role"
        || ARIA.contains(&name)
    {
        return Some(AttributeType::String);
    }
    generated::GLOBALS
        .iter()
        .chain(element(tag).into_iter().flat_map(|element| element.attrs))
        .find_map(|(key, typ)| (*key == name).then_some(*typ))
        .or_else(|| {
            (custom(tag) && !name.starts_with("on") && !name.starts_with("aria-"))
                .then_some(AttributeType::String)
        })
}

// WAI-ARIA 1.3 attribute vocabulary: https://w3c.github.io/aria/#state_prop_def
const ARIA: &[&str] = &[
    "aria-activedescendant",
    "aria-atomic",
    "aria-autocomplete",
    "aria-braillelabel",
    "aria-brailleroledescription",
    "aria-busy",
    "aria-checked",
    "aria-colcount",
    "aria-colindex",
    "aria-colindextext",
    "aria-colspan",
    "aria-controls",
    "aria-current",
    "aria-describedby",
    "aria-description",
    "aria-details",
    "aria-disabled",
    "aria-dropeffect",
    "aria-errormessage",
    "aria-expanded",
    "aria-flowto",
    "aria-grabbed",
    "aria-haspopup",
    "aria-hidden",
    "aria-invalid",
    "aria-keyshortcuts",
    "aria-label",
    "aria-labelledby",
    "aria-level",
    "aria-live",
    "aria-modal",
    "aria-multiline",
    "aria-multiselectable",
    "aria-orientation",
    "aria-owns",
    "aria-placeholder",
    "aria-posinset",
    "aria-pressed",
    "aria-readonly",
    "aria-relevant",
    "aria-required",
    "aria-roledescription",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowindextext",
    "aria-rowspan",
    "aria-selected",
    "aria-setsize",
    "aria-sort",
    "aria-valuemax",
    "aria-valuemin",
    "aria-valuenow",
    "aria-valuetext",
];

/// Event property spellings follow DOM, UI Events, Pointer Events and Input
/// Events. Every accepted handler receives a closed record of readable fields.
pub(crate) fn event(name: &str) -> Option<&'static str> {
    Some(match name {
        "onClick" | "onDblClick" | "onAuxClick" | "onContextMenu" | "onMouseDown" | "onMouseUp"
        | "onMouseMove" | "onMouseEnter" | "onMouseLeave" | "onMouseOver" | "onMouseOut" => {
            "MouseEvent"
        }
        "onPointerDown"
        | "onPointerUp"
        | "onPointerMove"
        | "onPointerEnter"
        | "onPointerLeave"
        | "onPointerOver"
        | "onPointerOut"
        | "onPointerCancel"
        | "onGotPointerCapture"
        | "onLostPointerCapture" => "PointerEvent",
        "onKeyDown" | "onKeyUp" | "onKeyPress" => "KeyboardEvent",
        "onInput" | "onBeforeInput" => "InputEvent",
        "onWheel" => "WheelEvent",
        "onFocus" | "onBlur" | "onFocusIn" | "onFocusOut" => "FocusEvent",
        "onSubmit"
        | "onReset"
        | "onChange"
        | "onInvalid"
        | "onLoad"
        | "onError"
        | "onAbort"
        | "onScroll"
        | "onScrollEnd"
        | "onResize"
        | "onSelect"
        | "onCancel"
        | "onClose"
        | "onToggle"
        | "onBeforeToggle"
        | "onPlay"
        | "onPause"
        | "onEnded"
        | "onPlaying"
        | "onWaiting"
        | "onCanPlay"
        | "onCanPlayThrough"
        | "onDurationChange"
        | "onEmptied"
        | "onLoadedData"
        | "onLoadedMetadata"
        | "onLoadStart"
        | "onProgress"
        | "onRateChange"
        | "onSeeked"
        | "onSeeking"
        | "onStalled"
        | "onSuspend"
        | "onTimeUpdate"
        | "onVolumeChange"
        | "onCopy"
        | "onCut"
        | "onPaste"
        | "onDrag"
        | "onDragStart"
        | "onDragEnd"
        | "onDragEnter"
        | "onDragLeave"
        | "onDragOver"
        | "onDrop"
        | "onAnimationStart"
        | "onAnimationEnd"
        | "onAnimationIteration"
        | "onAnimationCancel"
        | "onTransitionStart"
        | "onTransitionEnd"
        | "onTransitionRun"
        | "onTransitionCancel" => "Event",
        _ => return None,
    })
}
