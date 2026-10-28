//! Accessible name and role for every component (issue #3, finding UI-001).
//!
//! iced 0.14 passes nothing to a screen reader yet (see
//! `design/schermlezerproef`). The library already records, for each control,
//! what a screen reader will need: a [`Role`], a name, a value and a [`State`].
//! Components in `design` attach a [`Node`] themselves from their label; icon
//! buttons and title-bar buttons take a required name. Apps name their own
//! controls with [`node`] or [`named`].
//!
//! The nodes travel through iced's operation system, like the keyboard focus.
//! [`Collect`] gathers them with their bounds, and lists every control in the Tab
//! route that has no name, so a test can demand that every control is named.
//! A bridge to AccessKit can later read the same nodes, without apps changing.
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::operation::{Focusable, Operation};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{overlay, Clipboard, Shell, Widget};
use iced::{mouse, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

/// What a control is, in the terms of UI Automation and AccessKit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Button,
    /// A button that stays pressed, such as Raster.
    ToggleButton,
    CheckBox,
    /// On/off switch; Windows shows it as a toggle button.
    Switch,
    RadioButton,
    TextField,
    ComboBox,
    Slider,
    ProgressBar,
    Tab,
    TreeItem,
    Row,
    MenuItem,
    /// A link in a sidebar or file page: moves to another page.
    Link,
    Dialog,
    /// The name of the control inside it, as `<label>` on the web: a property
    /// row names its field this way.
    Label,
    /// A message that asks for attention, such as an error banner.
    Alert,
    /// A message that does not interrupt, such as a toast or the status bar.
    Status,
    Group,
}

impl Role {
    /// Whether this role is a container: its name describes an area, not the
    /// controls in it, so it does not name them.
    pub fn is_container(self) -> bool {
        matches!(self, Role::Group | Role::Dialog | Role::Alert | Role::Status)
    }
}

/// The parts of a control's state a screen reader announces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    pub checked: Option<bool>,
    pub selected: bool,
    pub expanded: Option<bool>,
    pub disabled: bool,
    pub invalid: bool,
    /// The current page or step in a sidebar or step bar.
    pub current: bool,
}

/// What a screen reader gets for one control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub role: Role,
    /// The name it reads, such as "Opslaan". Never empty for a control.
    pub name: String,
    /// The value, such as the text of a field or "62 %".
    pub value: Option<String>,
    /// More detail, such as a shortcut or the error of a field.
    pub description: Option<String>,
    pub state: State,
}

impl Node {
    pub fn new(role: Role, name: impl Into<String>) -> Self {
        Self { role, name: name.into(), value: None, description: None, state: State::default() }
    }
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
    pub fn checked(mut self, checked: bool) -> Self {
        self.state.checked = Some(checked);
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.state.selected = selected;
        self
    }
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.state.expanded = Some(expanded);
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.state.disabled = disabled;
        self
    }
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.state.invalid = invalid;
        self
    }
    pub fn current(mut self, current: bool) -> Self {
        self.state.current = current;
        self
    }
}

/// Gives `content` an accessible role, name, value and state. Put it around the
/// focusable control, not inside it.
pub fn node<'a, M: 'a>(content: impl Into<Element<'a, M>>, node: Node) -> Element<'a, M> {
    Element::new(Accessible { content: content.into(), node })
}

/// Shorthand for [`node`] with only a role and a name.
pub fn named<'a, M: 'a>(content: impl Into<Element<'a, M>>, role: Role, name: impl Into<String>) -> Element<'a, M> {
    node(content, Node::new(role, name))
}

struct Accessible<'a, M> {
    content: Element<'a, M>,
    node: Node,
}

impl<M> Widget<M, Theme, Renderer> for Accessible<'_, M> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let mut node = self.node.clone();
        operation.custom(None, layout.bounds(), &mut node);
        operation.traverse(&mut |operation| {
            self.content.as_widget_mut().operate(tree, layout, renderer, operation);
        });
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
    }
    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}

/// One node found by [`Collect`], with its place on screen and how deep it sits
/// in other nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub node: Node,
    pub bounds: Rectangle,
    pub depth: usize,
}

/// Gathers the accessible nodes of a screen, in reading order, and the controls
/// in the Tab route that no named node covers.
///
/// A node names the controls within its bounds, unless its role is a container
/// ([`Role::is_container`]). Bounds rather than nesting decide this, because
/// `focus::group` walks its items with its own operation.
#[derive(Debug, Default)]
pub struct Collect {
    pub nodes: Vec<Found>,
    /// Bounds of focusable controls without a name.
    pub unnamed: Vec<Rectangle>,
}

impl Collect {
    /// The nodes with a name, as "Rol: naam".
    pub fn summary(&self) -> Vec<String> {
        self.nodes.iter().filter(|f| !f.node.name.is_empty()).map(|f| format!("{:?}: {}", f.node.role, f.node.name)).collect()
    }

    fn names(&self, bounds: Rectangle) -> bool {
        let inside = |outer: Rectangle| outer.x <= bounds.x + 0.5 && outer.y <= bounds.y + 0.5 && outer.x + outer.width >= bounds.x + bounds.width - 0.5 && outer.y + outer.height >= bounds.y + bounds.height - 0.5;
        self.nodes.iter().any(|f| !f.node.role.is_container() && !f.node.name.trim().is_empty() && inside(f.bounds))
    }
}

impl Operation for Collect {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn custom(&mut self, _id: Option<&iced::widget::Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        if let Some(node) = state.downcast_ref::<Node>() {
            let depth = self.nodes.iter().filter(|f| f.bounds.contains(bounds.center())).count();
            self.nodes.push(Found { node: node.clone(), bounds, depth });
        }
    }
    fn focusable(&mut self, _id: Option<&iced::widget::Id>, bounds: Rectangle, _state: &mut dyn Focusable) {
        // A node is reported before its content, so the one around this control is known.
        if !self.names(bounds) {
            self.unnamed.push(bounds);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::focus::focusable;
    use crate::design::{StatusbarSetting, ThemeId, WorkspaceSetting};
    use crate::design::Look;
    use iced::advanced::layout::Limits;
    use iced::widget::{button, column, text};

    fn collect(mut element: Element<'static, ()>) -> Collect {
        let renderer = iced_tiny_skia::Renderer::new(crate::fonts::BODY, iced::Pixels(12.0));
        #[cfg(feature = "gpu-renderer")]
        let renderer = Renderer::Secondary(renderer);
        let mut tree = Tree::new(&element);
        let node = element.as_widget_mut().layout(&mut tree, &renderer, &Limits::new(Size::ZERO, Size::new(400.0, 300.0)));
        let mut found = Collect::default();
        element.as_widget_mut().operate(&mut tree, Layout::new(&node), &renderer, &mut found);
        found
    }

    #[test]
    fn a_control_needs_a_name_of_its_own() {
        let look = Look::new(ThemeId::Light, WorkspaceSetting::Theme, StatusbarSetting::Theme);
        let control = || focusable(look, button(text("?")).on_press(()), true, 0.0);
        // Without a node, and inside a named group, the control has no name.
        let found = collect(column![control(), named(column![control()], Role::Group, "Paneel")].into());
        assert_eq!(found.unnamed.len(), 2);
        // A node or a label names it.
        let found = collect(column![named(control(), Role::Button, "Opslaan"), named(control(), Role::Label, "Prijs")].into());
        assert!(found.unnamed.is_empty());
        assert_eq!(found.summary(), ["Button: Opslaan", "Label: Prijs"]);
        // An empty name is no name.
        let found = collect(named(control(), Role::Button, " ").into());
        assert_eq!(found.unnamed.len(), 1);
    }
}
