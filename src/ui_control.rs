//! The window driven as a person drives it: what controls it shows, by role and name (the
//! accessible names every control has), and a click on one, text typed into a field, a key
//! pressed. A click goes through the same widget a mouse click reaches, on a headless copy of the
//! window at its own size, so it sends the message that widget sends, or nothing when the
//! control does not answer: that is how a remote client knows a button works.
//!
//! The remote commands `ui_controls`, `ui_press`, `ui_type` and `ui_key` come here
//! ([`crate::shell::Studio::ui_command`]).

use openaec_ui::design::a11y::{Collect, Found, Role};
use openaec_ui::iced::advanced::layout::{self, Layout};
use openaec_ui::iced::advanced::widget::Tree;
use openaec_ui::iced::{self, Element, Rectangle, Size};
use serde_json::{json, Value};

/// The controls of a view laid out at `size`, in reading order.
pub fn controls<M>(mut element: Element<'_, M>, size: Size) -> Vec<Found> {
    let mut tree = Tree::new(&element);
    let renderer = iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(openaec_ui::fonts::BODY, iced::Pixels(12.0)));
    let limits = layout::Limits::new(Size::ZERO, size);
    let node = element.as_widget_mut().layout(&mut tree, &renderer, &limits);
    let mut collect = Collect::default();
    element.as_widget_mut().operate(&mut tree, Layout::new(&node), &renderer, &mut collect);
    collect.nodes
}

/// What a client is told of a control.
pub fn describe(k: usize, f: &Found) -> Value {
    let s = f.node.state;
    let mut v = json!({
        "index": k,
        "role": format!("{:?}", f.node.role),
        "name": f.node.name,
        "bounds": [f.bounds.x.round(), f.bounds.y.round(), f.bounds.width.round(), f.bounds.height.round()],
    });
    if let Some(value) = &f.node.value {
        v["value"] = json!(value);
    }
    if let Some(d) = &f.node.description {
        v["description"] = json!(d);
    }
    if s.disabled {
        v["disabled"] = json!(true);
    }
    if let Some(c) = s.checked {
        v["checked"] = json!(c);
    }
    if s.selected {
        v["selected"] = json!(true);
    }
    if let Some(e) = s.expanded {
        v["expanded"] = json!(e);
    }
    v
}

/// Whether a control is one a person can act on (not an area or a message).
pub fn is_control(f: &Found) -> bool {
    !f.node.role.is_container() && !matches!(f.node.role, Role::Label) && !f.node.name.trim().is_empty()
}

/// The control a command names: by `index` from `ui_controls`, or by `name` (exact, else the one
/// whose name holds it, in any case), with `role` and the `nth` of those that match (from 0).
pub fn find<'a>(found: &'a [Found], args: &Value) -> Result<(usize, &'a Found), String> {
    if let Some(i) = args.get("index").and_then(Value::as_u64) {
        let i = i as usize;
        return found.get(i).map(|f| (i, f)).ok_or_else(|| format!("no control {i}; ui_controls lists {}", found.len()));
    }
    let name = args.get("name").and_then(Value::as_str).ok_or("give the control's name or index")?.trim().to_lowercase();
    let role = args.get("role").and_then(Value::as_str).map(str::to_lowercase);
    let nth = args.get("nth").and_then(Value::as_u64).unwrap_or(0) as usize;
    let fits_role = |f: &Found| role.as_deref().is_none_or(|r| format!("{:?}", f.node.role).to_lowercase() == r);
    let candidates = |exact: bool| -> Vec<(usize, &'a Found)> {
        found
            .iter()
            .enumerate()
            .filter(|(_, f)| is_control(f) && fits_role(f))
            .filter(|(_, f)| {
                let n = f.node.name.trim().to_lowercase();
                if exact { n == name } else { n.contains(&name) }
            })
            .collect()
    };
    let exact = candidates(true);
    let list = if exact.is_empty() { candidates(false) } else { exact };
    if list.is_empty() {
        let near: Vec<String> = found.iter().filter(|f| is_control(f)).map(|f| f.node.name.clone()).filter(|n| n.to_lowercase().chars().take(3).any(|c| name.contains(c))).take(12).collect();
        return Err(format!("no control named '{name}'; some there are: {near:?}"));
    }
    list.get(nth).copied().ok_or_else(|| format!("{} controls are named '{name}', not {}", list.len(), nth + 1))
}

/// Where a click on a control lands: the middle of its part that is on screen.
pub fn aim(bounds: Rectangle, size: Size) -> Option<iced::Point> {
    let screen = Rectangle::new(iced::Point::ORIGIN, size);
    let seen = bounds.intersection(&screen)?;
    (seen.width >= 1.0 && seen.height >= 1.0).then(|| seen.center())
}
