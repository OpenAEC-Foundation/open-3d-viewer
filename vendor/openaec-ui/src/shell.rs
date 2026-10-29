use crate::{fonts, icons::Icon, model::*, widgets, Language, Theme};
use iced::{widget::{button, column, container, mouse_area, pane_grid, row, scrollable, slider, stack, text, tooltip, Row, Space}, Element, Fill, Padding};

/// Application-owned contents. Shell layout and styling stay inside the crate.
pub struct Slots<'a, M> {
    pub title: String,
    pub mode: ChromeMode,
    pub tabs: Vec<RibbonTab<M>>,
    pub documents: Vec<Document>,
    pub active_document: Option<String>,
    pub file_commands: Vec<Command<M>>,
    pub file_active_label: Option<String>,
    pub quick_commands: Vec<Command<M>>,
    pub status: String,
    pub content: Box<dyn Fn(Region) -> Element<'a, Message<M>> + 'a>,
    pub overlay: Option<Element<'a, Message<M>>>,
}
fn horizontal<'a,M:'a>(content: impl Into<Element<'a,M>>) -> Element<'a,M> {
    scrollable(content).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new())).into()
}
pub fn shell<'a, M: Clone + 'a>(state: &'a ShellState, slots: Slots<'a,M>) -> Element<'a,Message<M>> {
    let theme = state.preferences.theme;
    let t = theme.tokens(); let l = state.preferences.language;
    let title = mouse_area(container(text(slots.title).font(fonts::MEDIUM).size(12).color(t.accent)).width(Fill).height(u32::from(metrics::TITLE)).center(Fill))
        .on_press(Message::Ui(UiEvent::DragWindow));
    let mut quick = Row::new().spacing(1);
    for command in &slots.quick_commands {
        let hint=format!("{}{}",command.label,command.shortcut.as_ref().map(|s|format!(" ({s})")).unwrap_or_default());
        quick=quick.push(widgets::title_action(
            command.icon.view(16,if command.enabled {t.accent} else {t.text_faint}).into(),
            hint,
            command.enabled.then(||Message::Command(command.message.clone())),
            theme,
        ));
    }
    let titlebar = row![
        container(Icon::Cube.view(18,t.accent)).padding([0,8]), quick, title,
        widgets::title_action(Icon::Settings.view(16,t.text).into(),l.text("Instellingen","Settings"),Some(Message::Ui(UiEvent::Settings(true))),theme),
    ];
    // In the browser the page is the window: no minimize, maximize or close.
    #[cfg(not(target_arch = "wasm32"))]
    let titlebar = titlebar.extend([
        widgets::title_action(Icon::Minimize.view(16,t.text).into(),l.text("Minimaliseren","Minimize"),Some(Message::Ui(UiEvent::Minimize)),theme),
        widgets::title_action(Icon::Maximize.view(16,t.text).into(),l.text("Maximaliseren","Maximize"),Some(Message::Ui(UiEvent::Maximize)),theme),
        widgets::title_action(Icon::Close.view(16,t.text).into(),l.text("Sluiten","Close"),Some(Message::Ui(UiEvent::CloseWindow)),theme),
    ]);
    let titlebar = container(titlebar.align_y(iced::Alignment::Center)).height(u32::from(metrics::TITLE)).style(move |_| container::Style::default().background(t.bg_lighter).color(t.text));
    let mut tabs = Row::new().spacing(if slots.mode == ChromeMode::CanvasRibbon { 6 } else { 2 });
    if !matches!(slots.mode, ChromeMode::TopNav | ChromeMode::CanvasRibbon | ChromeMode::Plain) {
        let label = l.text("Bestand", "File");
        let anchor = widgets::ribbon_tab(
            text(label).font(fonts::STRONG).size(12).into(),
            Message::Ui(UiEvent::Backstage(!state.backstage)),
            label.chars().count() as f32 * 7.0 + 32.0,
            widgets::RibbonTabKind::File,
            theme,
        );
        tabs = tabs.push(widgets::popover(anchor, file_menu(&slots.file_commands, theme, l),
            state.backstage, Message::Ui(UiEvent::Backstage(false))));
    }
    let active = slots.tabs.iter().find(|tab| tab.id == state.active_tab).or(slots.tabs.first());
    for tab in &slots.tabs {
        let selected = !state.backstage && active.is_some_and(|a| a.id == tab.id);
        tabs = tabs.push(widgets::ribbon_tab(
            text(tab.label.clone()).font(fonts::MEDIUM).size(12).into(),
            Message::Ui(UiEvent::Tab(tab.id.clone())),
            tab.label.chars().count() as f32 * 7.0 + 32.0,
            widgets::RibbonTabKind::Regular { active: selected },
            theme,
        ));
    }
    let tabs = widgets::ribbon_tab_strip(horizontal(tabs), theme);
    let ribbon = active.map(|tab| ribbon_body(theme, &tab.groups).map(Message::Command))
        .unwrap_or_else(|| Space::new().height(u32::from(metrics::RIBBON)).into());
    let has_documents = !slots.documents.is_empty();
    let mut documents = Row::new().spacing(1).align_y(iced::Alignment::End);
    for doc in slots.documents {
        let selected=slots.active_document.as_ref() == Some(&doc.id);
        let label=format!("{}{}",doc.title,if doc.dirty {" •"} else {""});
        let close = button(text("×").size(12))
            .on_press(Message::Document(DocumentEvent::CloseRequested(doc.id.clone())))
            .style(move |_, status| widgets::document_close_style(theme, selected, status))
            .padding([0, 4]);
        let content = row![text(label).size(11), close]
            .spacing(6)
            .align_y(iced::Alignment::Center);
        let tab = button(content)
            .on_press(Message::Document(DocumentEvent::Activate(doc.id)))
            .height(24)
            .padding(Padding { top: 0.0, bottom: 0.0, left: 10.0, right: 4.0 })
            .style(move |_, status| widgets::document_tab_style(theme, selected, status));
        documents = documents.push(widgets::document_tab_bottom_gap(theme, tab.into()));
    }
    let document_bar=crate::frame::document_bar(theme, horizontal(documents));
    let compact_sidebar = (slots.mode == ChromeMode::SidebarCompact)
        .then(|| (slots.content)(Region::Explorer));
    let body: Element<'a,Message<M>> = if matches!(slots.mode, ChromeMode::TopNav | ChromeMode::Plain) {
        (slots.content)(Region::Workspace)
    } else if slots.mode == ChromeMode::RibbonWorkspace {
        (slots.content)(Region::Workspace)
    } else if slots.mode == ChromeMode::RibbonSidebar {
        row![
            container((slots.content)(Region::Explorer)).width(288).height(Fill),
            container((slots.content)(Region::Workspace)).width(Fill).height(Fill),
        ].height(Fill).width(Fill).into()
    } else if let ChromeMode::ThreePaneRibbon { left, right } = slots.mode {
        let mut panes = Row::new().spacing(2);
        if left {
            panes = panes.push(container((slots.content)(Region::Explorer)).width(iced::FillPortion(1)).height(Fill));
        }
        panes = panes.push(container((slots.content)(Region::Workspace))
            .width(iced::FillPortion(if left && right { 2 } else if left || right { 3 } else { 4 })).height(Fill));
        if right {
            panes = panes.push(container((slots.content)(Region::Properties)).width(iced::FillPortion(1)).height(Fill));
        }
        panes.height(Fill).width(Fill).into()
    } else if let ChromeMode::ThreePaneFixed { left_width, right_width } = slots.mode {
        row![
            container((slots.content)(Region::Explorer)).width(u32::from(left_width)).height(Fill),
            container((slots.content)(Region::Workspace)).width(Fill).height(Fill),
            container((slots.content)(Region::Properties)).width(u32::from(right_width)).height(Fill),
        ].height(Fill).width(Fill).into()
    } else if slots.mode == ChromeMode::CanvasRibbon {
        row![
            container((slots.content)(Region::Explorer)).width(255).height(Fill),
            container((slots.content)(Region::Workspace)).width(Fill).height(Fill),
        ].height(Fill).width(Fill).into()
    } else if matches!(slots.mode, ChromeMode::Workflow | ChromeMode::Sidebar) {
        row![
            container((slots.content)(Region::Explorer)).width(260).height(Fill),
            container((slots.content)(Region::Workspace)).width(Fill).height(Fill),
        ].height(Fill).width(Fill).into()
    } else if slots.mode == ChromeMode::SidebarCompact {
        (slots.content)(Region::Workspace)
    } else {
        pane_grid::PaneGrid::new(&state.panes, move |_, region, _| pane_grid::Content::new((slots.content)(*region)))
            .spacing(2).on_resize(6,|e|Message::Ui(UiEvent::Resize(e))).height(Fill).into()
    };
    let mut status_row = Row::new().push(text(slots.status).size(11).width(Fill));
    if slots.mode == ChromeMode::Ribbon {
        status_row = status_row
            .push(widgets::status_icon_button(l.text("Verkenner","Explorer"),Icon::PanelLeft,Message::Ui(UiEvent::ToggleLeft),theme))
            .push(widgets::status_icon_button(l.text("Eigenschappen","Properties"),Icon::PanelRight,Message::Ui(UiEvent::ToggleRight),theme));
    }
    let status = crate::frame::status_bar(
        theme,
        status_row.align_y(iced::Alignment::Center).height(Fill).into(),
        8,
    );
    let bands = if slots.mode == ChromeMode::SidebarCompact {
        crate::frame::sidebar_frame(
            crate::frame::SidebarFrameProfile::WORKSPACE_COMPOSER,
            titlebar.into(),
            compact_sidebar.expect("compact sidebar"),
            body,
            status.into(),
        )
    } else if slots.mode == ChromeMode::Plain {
        crate::frame::workflow_frame(titlebar.into(), None, body, status.into())
    } else if slots.mode == ChromeMode::TopNav {
        crate::frame::frame(crate::frame::FrameSlots {
            titlebar: titlebar.into(), tabs: tabs.into(), ribbon: None,
            documents: None, content: body, status: status.into(),
        })
    } else if matches!(slots.mode, ChromeMode::Workflow | ChromeMode::Sidebar) {
        crate::frame::workflow_frame(titlebar.into(), (slots.mode == ChromeMode::Workflow && has_documents).then_some(document_bar.into()), body, status.into())
    } else {
        crate::frame::frame(crate::frame::FrameSlots {
            titlebar: titlebar.into(),
            tabs: tabs.into(),
            ribbon: Some(ribbon.into()),
            documents: has_documents.then_some(document_bar.into()),
            content: body,
            status: status.into(),
        })
    };
    let base: Element<'a,Message<M>>=container(bands).height(Fill).width(Fill).style(move |_| widgets::surface(theme)).into();
    let mut layers = vec![base];
    if let Some(overlay) = slots.overlay { layers.push(overlay); }
    if state.settings {
        layers.push(settings(state));
        if state.settings_reset_confirm { layers.push(reset_confirmation(state)); }
    }
    if layers.len() == 1 { layers.pop().unwrap() }
    else { stack(layers).width(Fill).height(Fill).into() }
}

/// A standard File menu keeps the model in view while offering direct actions.
fn file_menu<'a, M: Clone + 'a>(commands: &[Command<M>], theme: Theme, l: Language) -> Element<'a, Message<M>> {
    let t = theme.tokens();
    let mut items = column![].spacing(2);
    for command in commands {
        let label = row![command.icon.view(18, if command.enabled { t.text } else { t.text_faint }),
            text(command.label.clone()).size(13), Space::new().width(Fill),
            text(command.shortcut.clone().unwrap_or_default()).size(11).color(t.text_secondary)]
            .spacing(10).align_y(iced::Alignment::Center);
        items = items.push(button(label).width(Fill).padding([9, 10])
            .on_press_maybe(command.enabled.then(|| Message::FileCommand(command.message.clone())))
            .style(move |_, status| widgets::ribbon_button_style(theme, false, status)));
    }
    items = items.push(iced::widget::rule::horizontal(1));
    items = items.push(button(row![Icon::Settings.view(18, t.text), text(l.text("Instellingen…", "Settings…")).size(13)]
        .spacing(10).align_y(iced::Alignment::Center))
        .width(Fill).padding([9, 10]).on_press(Message::Ui(UiEvent::Settings(true)))
        .style(move |_, status| widgets::ribbon_button_style(theme, false, status)));
    #[cfg(not(target_arch = "wasm32"))]
    { items = items.push(button(text(l.text("Afsluiten", "Exit")).size(13))
        .width(Fill).padding([9, 10]).on_press(Message::Ui(UiEvent::CloseWindow))
        .style(move |_, status| widgets::ribbon_button_style(theme, false, status))); }
    container(items).width(280).padding(6).style(move |_| container::Style {
        background: Some(t.surface.into()), text_color: Some(t.text),
        border: iced::Border { color: t.border, width: 1.0, radius: 6.0.into() },
        shadow: theme.dialog_shadow(), ..Default::default()
    }).into()
}

fn settings<'a,M:Clone+'a>(state:&'a ShellState) -> Element<'a,Message<M>> {
    let p=&state.preferences; let l=p.language;
    let heading = |label: &'static str| widgets::settings_section_heading(label,p.theme);
    let form: Element<'a, Message<M>> = match state.settings_page {
        SettingsPage::General => column![
            heading(l.text("Algemeen","General")),
            text(l.text("Stel de taal en weergave van de app in.", "Choose the app language and appearance.")).size(12).color(p.theme.tokens().text_secondary),
            text(l.text("Taal","Language")).size(13),
            row![
                button(text("Nederlands").size(13)).width(Fill).padding([8, 12])
                    .on_press(Message::Ui(UiEvent::Language(Language::Dutch)))
                    .style(move |_, status| widgets::ribbon_button_style(p.theme, p.language == Language::Dutch, status)),
                button(text("English").size(13)).width(Fill).padding([8, 12])
                    .on_press(Message::Ui(UiEvent::Language(Language::English)))
                    .style(move |_, status| widgets::ribbon_button_style(p.theme, p.language == Language::English, status)),
            ].spacing(6),
            text(l.text("Thema", "Theme")).size(13),
            widgets::settings_theme_picker(
                Theme::ALL.into_iter().map(|theme| (theme, theme.to_string(), Message::Ui(UiEvent::Theme(theme)))).collect(),
                p.theme, p.theme, state.settings_theme_open,
                Message::Ui(UiEvent::SettingsThemeMenu(!state.settings_theme_open)),
            ),
            text(format!("{}: {:.0}%",l.text("Interfacegrootte","Interface size"),p.scale*100.0)).size(13),
            slider(0.75..=2.0,p.scale,|scale|Message::Ui(UiEvent::Scale(scale)))
                .step(0.05).style(move |_,status|widgets::slider_style(p.theme,status)),
        ].spacing(10).into(),
        SettingsPage::Appearance => column![
            heading(l.text("Thema","Theme")),
            row![
                Space::new().width(Fill),
                widgets::settings_theme_picker(
                    Theme::ALL.into_iter().map(|theme| (
                        theme, theme.to_string(), Message::Ui(UiEvent::Theme(theme)),
                    )).collect(),
                    p.theme, p.theme, state.settings_theme_open,
                    Message::Ui(UiEvent::SettingsThemeMenu(!state.settings_theme_open)),
                ),
            ],
        ].spacing(10).into(),
        SettingsPage::About => column![
            heading(l.text("Over","About")),
            text("OpenAEC UI").font(fonts::STRONG).size(12),
            text(l.text("Gedeelde native Rust-interface voor OpenAEC-tools.","Shared native Rust interface for OpenAEC tools.")).size(11),
        ].spacing(10).into(),
    };
    let sidebar=column![
        widgets::settings_tab(l.text("Algemeen","General"),state.settings_page==SettingsPage::General,Message::Ui(UiEvent::SettingsPage(SettingsPage::General)),p.theme),
        widgets::settings_tab(l.text("Uiterlijk","Appearance"),state.settings_page==SettingsPage::Appearance,Message::Ui(UiEvent::SettingsPage(SettingsPage::Appearance)),p.theme),
        widgets::settings_tab(l.text("Over","About"),state.settings_page==SettingsPage::About,Message::Ui(UiEvent::SettingsPage(SettingsPage::About)),p.theme),
    ].spacing(2);
    let footer=row![
        widgets::dialog_button(l.text("Standaardwaarden herstellen","Reset to Defaults"),move |_,status|widgets::secondary_button_style(p.theme,status))
            .on_press(Message::Ui(UiEvent::SettingsResetPrompt(true))),
        Space::new().width(Fill),
        widgets::dialog_button(l.text("Annuleren","Cancel"),move |_,status|widgets::secondary_button_style(p.theme,status))
            .on_press(Message::Ui(UiEvent::Settings(false))),
        widgets::dialog_button(l.text("Opslaan","Save"),move |_,status|widgets::primary_button_style(p.theme,status))
            .on_press(Message::Ui(UiEvent::SettingsSave)),
    ].spacing(6).align_y(iced::Alignment::Center);
    widgets::settings_modal(
        l.text("Instellingen","Settings"), sidebar.into(), form, footer.into(),
        Message::Ui(UiEvent::Settings(false)), p.theme,
    )
}
fn reset_confirmation<'a,M:Clone+'a>(state:&'a ShellState) -> Element<'a,Message<M>> {
    let l=state.preferences.language;
    widgets::settings_reset_confirmation(
        l.text("Standaardwaarden herstellen","Reset to Defaults"),
        l.text("Weet je zeker dat je alle instellingen naar hun standaardwaarden wilt herstellen?",
            "Are you sure you want to reset all settings to their default values?"),
        l.text("Annuleren","Cancel"),
        l.text("Standaardwaarden herstellen","Reset to Defaults"),
        Message::Ui(UiEvent::SettingsResetPrompt(false)),
        Message::Ui(UiEvent::SettingsReset),
        state.preferences.theme,
    )
}

/// Compact command groups: up to three labelled 16 px actions per column.
/// Owns labels and messages, so callers can share one command definition.
pub fn ribbon_body<'a, M: Clone + 'a>(theme: Theme, groups: &[RibbonGroup<M>]) -> Element<'a, M> {
    let t = theme.tokens();
    let mut strip = Row::new().spacing(0);
    for group in groups {
        let mut columns = Row::new().spacing(2);
        let mut tools_width = 0.0f32;
        for chunk in group.commands.chunks(3) {
            let width = chunk.iter().map(|command| {
                command.control.as_ref().map_or_else(
                    || (command.label.chars().count() as f32 * 5.8 + 36.0).clamp(62.0, 164.0),
                    |control| control.width(),
                )
            }).fold(0.0f32, f32::max);
            let mut actions = column![].spacing(0);
            for command in chunk {
                if let Some(control) = &command.control {
                    actions = actions.push(widgets::ribbon_control(theme, command.label.clone(), control.clone()));
                    continue;
                }
                let selected = command.selected;
                let color = if !command.enabled { t.text_faint } else if selected { t.ribbon_icon_active } else { t.text };
                let hint = format!("{}{}", command.label, command.shortcut.as_ref().map(|s| format!(" ({s})")).unwrap_or_default());
                let action = button(row![command.icon.view(16, color), text(command.label.clone()).font(fonts::BODY).size(11)]
                    .spacing(4).align_y(iced::Alignment::Center))
                    .padding([2, 4]).width(width).height(22)
                    .on_press_maybe(command.enabled.then(|| command.message.clone()))
                    .style(move |_, status| widgets::ribbon_button_style(theme, selected, status));
                actions = actions.push(widgets::themed_tooltip(action, hint, tooltip::Position::Bottom, theme));
            }
            tools_width += width;
            columns = columns.push(actions);
        }
        tools_width += group.commands.len().div_ceil(3).saturating_sub(1) as f32 * 2.0;
        let caption = group.label.chars().count() as f32 * 6.0 + 8.0;
        strip = strip.push(widgets::ribbon_group(theme, group.label.clone(), columns.into(), tools_width.max(caption) + 13.0));
    }
    container(scrollable(strip).direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::new().width(3).scroller_width(3).margin(0),
    )))
        .height(u32::from(metrics::RIBBON))
        .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 2.0 })
        .style(move |_| container::Style::default().background(t.surface))
        .into()
}
