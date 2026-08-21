use std::ops::Range;
use std::sync::Arc;

use collections::HashMap;
use editor::actions::{
    FindAllReferences, GoToDeclaration, GoToDefinition, GoToImplementation, GoToTypeDefinition,
};
use editor::hover_links::{FindAllReferencesAt, HoverLink, OpenDefinitionLocations};
use editor::{
    Editor, EditorSettings, GotoDefinitionKind, Navigated, NavigationRequest, OpenResultsIn,
};
use file_icons::FileIcons;
use fuzzy::StringMatchCandidate;
use gpui::{
    AnyElement, App, AppContext, AsyncWindowContext, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, HighlightStyle, StyledText, Subscription, Task, TextStyle, WeakEntity,
    prelude::*,
};
use language::{Buffer, HighlightId, LanguageAwareStyling};
use picker::{Picker, PickerDelegate};
use project::{Location, LocationLink, Project, ProjectPath};
use settings::{GoToDefinitionFallback, Settings as _};
use text::{Anchor, Point};
use theme_settings::ThemeSettings;
use ui::{Divider, FluentBuilder};
use ui::{ListItem, ListItemSpacing, prelude::*};

use workspace::item::ItemSettings;
use workspace::notifications::NotificationId;
use workspace::pane::NavigationEntry;
use workspace::{ModalView, Toast, Workspace};

pub fn init(cx: &mut App) {
    cx.observe_new(register).detach();
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action_renderer(|div, _, _, cx| {
            div.capture_action(cx.listener(capture_definition_locations))
                .capture_action(cx.listener(capture_references))
        });
    })
    .detach();
}

/// Registers handlers for the navigation actions on each full editor. When the
/// action resolves to [`OpenResultsIn::Picker`], we open the filterable picker;
/// otherwise we `cx.propagate()` so the editor's own handler runs and builds a
/// multibuffer.
fn register(editor: &mut Editor, _window: Option<&mut Window>, cx: &mut Context<Editor>) {
    if !editor.lsp_data_enabled() {
        return;
    }
    let handle = cx.entity().downgrade();
    editor
        .register_action({
            let handle = handle.clone();
            move |action: &GoToDefinition, window, cx| {
                handle_nav_action(
                    action.open_results_in,
                    LspPickerKind::Definition,
                    &handle,
                    window,
                    cx,
                );
            }
        })
        .detach();
    editor
        .register_action({
            let handle = handle.clone();
            move |action: &GoToDeclaration, window, cx| {
                handle_nav_action(
                    action.open_results_in,
                    LspPickerKind::Declaration,
                    &handle,
                    window,
                    cx,
                );
            }
        })
        .detach();
    editor
        .register_action({
            let handle = handle.clone();
            move |action: &GoToImplementation, window, cx| {
                handle_nav_action(
                    action.open_results_in,
                    LspPickerKind::Implementation,
                    &handle,
                    window,
                    cx,
                );
            }
        })
        .detach();
    editor
        .register_action({
            let handle = handle.clone();
            move |action: &GoToTypeDefinition, window, cx| {
                handle_nav_action(
                    action.open_results_in,
                    LspPickerKind::TypeDefinition,
                    &handle,
                    window,
                    cx,
                );
            }
        })
        .detach();
    editor
        .register_action(move |action: &FindAllReferences, window, cx| {
            handle_nav_action(
                action.open_results_in,
                LspPickerKind::References,
                &handle,
                window,
                cx,
            );
        })
        .detach();
}

fn capture_definition_locations(
    workspace: &mut Workspace,
    action: &OpenDefinitionLocations,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if EditorSettings::get_global(cx).lsp_results_location != OpenResultsIn::Picker {
        return;
    }
    let navigation = &action.0.source;
    let Some(source) = source_editor(&navigation.editor, workspace, cx) else {
        return;
    };
    if !navigation.request.is_current(source.read(cx)) {
        cx.stop_propagation();
        return;
    }
    if action
        .0
        .locations
        .iter()
        .any(|location| location.buffer.read(cx).file().is_none())
    {
        return;
    }
    let matches = build_location_matches(&action.0.locations, cx);
    if matches.len() < 2 {
        return;
    }
    let kind = match action.0.kind {
        GotoDefinitionKind::Symbol => LspPickerKind::Definition,
        GotoDefinitionKind::Type => LspPickerKind::TypeDefinition,
        GotoDefinitionKind::Declaration => LspPickerKind::Declaration,
        GotoDefinitionKind::Implementation => LspPickerKind::Implementation,
    };
    cx.stop_propagation();
    LspLocationsPicker::present(
        kind,
        matches,
        navigation.editor.clone(),
        navigation.origin.clone(),
        workspace,
        window,
        cx,
    );
}

fn capture_references(
    workspace: &mut Workspace,
    action: &FindAllReferencesAt,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if EditorSettings::get_global(cx).lsp_results_location != OpenResultsIn::Picker {
        return;
    }
    let Some(source) = source_editor(&action.0.editor, workspace, cx) else {
        return;
    };
    if !action.0.request.is_current(source.read(cx)) {
        cx.stop_propagation();
        return;
    }
    cx.stop_propagation();
    LspLocationsPicker::open_with_request(
        LspPickerKind::References,
        action.0.editor.clone(),
        action.0.position,
        action.0.origin.clone(),
        action.0.request.clone(),
        workspace,
        window,
        cx,
    );
}

fn source_editor(
    editor: &WeakEntity<Editor>,
    workspace: &Workspace,
    cx: &App,
) -> Option<Entity<Editor>> {
    let editor = editor.upgrade()?;
    let source = editor.read(cx);
    if !source.lsp_data_enabled()
        || source.workspace()?.downgrade() != workspace.weak_handle()
        || Editor::containing_item(workspace, editor.entity_id(), cx).is_none()
    {
        return None;
    }
    Some(editor)
}

fn open_location(
    editor: &mut Editor,
    location: Location,
    origin: Option<NavigationEntry>,
    split: bool,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Task<anyhow::Result<Navigated>> {
    if let Some(origin) = origin {
        editor.navigate_to_hover_links(
            None,
            vec![HoverLink::Text(LocationLink {
                origin: None,
                target: location,
            })],
            Some(origin),
            split,
            window,
            cx,
        )
    } else {
        editor.open_location(location, split, window, cx)
    }
}

/// Either opens the picker for the editor, or propagates the action so the
/// editor's built-in (multibuffer) handler runs. A `None` argument falls back to
/// the `lsp_results_location` setting.
fn handle_nav_action(
    open_results_in: Option<OpenResultsIn>,
    kind: LspPickerKind,
    editor: &WeakEntity<Editor>,
    window: &mut Window,
    cx: &mut App,
) {
    let open_results_in =
        open_results_in.unwrap_or_else(|| EditorSettings::get_global(cx).lsp_results_location);
    if open_results_in != OpenResultsIn::Picker
        || !LspLocationsPicker::open_for_editor(kind, editor.clone(), window, cx)
    {
        cx.propagate();
    }
}

/// Runs the query for `kind` and builds the displayable, deduped matches.
async fn run_picker_matches(
    kind: LspPickerKind,
    editor: &WeakEntity<Editor>,
    workspace: &WeakEntity<Workspace>,
    project: &Entity<Project>,
    position: editor::Anchor,
    request: &NavigationRequest,
    cx: &mut AsyncWindowContext,
) -> Option<Vec<LocationMatch>> {
    let query = cx
        .update(|_, cx| {
            let workspace = workspace.upgrade()?;
            let source = source_editor(editor, workspace.read(cx), cx)?;
            if !request.is_current(source.read(cx)) {
                return None;
            }
            source.update(cx, |editor, cx| {
                kind.run_query(editor, project, position, cx)
            })
        })
        .ok()
        .flatten()?;
    let locations = query.await;
    cx.update(|_, cx| {
        let workspace = workspace.upgrade()?;
        let source = source_editor(editor, workspace.read(cx), cx)?;
        if !request.is_current(source.read(cx)) {
            return None;
        }
        match locations {
            Ok(locations) => Some(build_location_matches(&locations, cx)),
            Err(error) => {
                log::error!("LSP {kind:?} query failed: {error:#}");
                workspace.update(cx, |workspace, cx| workspace.show_error(error, cx));
                None
            }
        }
    })
    .ok()
    .flatten()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LspPickerKind {
    References,
    Definition,
    Declaration,
    Implementation,
    TypeDefinition,
}

impl LspPickerKind {
    fn placeholder(self) -> &'static str {
        match self {
            LspPickerKind::References => "Filter references…",
            LspPickerKind::Definition => "Filter definitions…",
            LspPickerKind::Declaration => "Filter declarations…",
            LspPickerKind::Implementation => "Filter implementations…",
            LspPickerKind::TypeDefinition => "Filter type definitions…",
        }
    }

    /// Message shown when the query produces no results, so the command does not
    /// appear to silently do nothing.
    fn empty_message(self) -> &'static str {
        match self {
            LspPickerKind::References => "No references found",
            LspPickerKind::Definition => "No definitions found",
            LspPickerKind::Declaration => "No declarations found",
            LspPickerKind::Implementation => "No implementations found",
            LspPickerKind::TypeDefinition => "No type definitions found",
        }
    }

    fn run_query(
        self,
        editor: &mut Editor,
        project: &Entity<Project>,
        position: editor::Anchor,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Vec<Location>>>> {
        match self {
            LspPickerKind::References => {
                editor.find_all_references_locations_at(project, position, cx)
            }
            LspPickerKind::Definition => {
                editor.definition_locations_of_kind_at(GotoDefinitionKind::Symbol, position, cx)
            }
            LspPickerKind::Declaration => editor.definition_locations_of_kind_at(
                GotoDefinitionKind::Declaration,
                position,
                cx,
            ),
            LspPickerKind::Implementation => editor.definition_locations_of_kind_at(
                GotoDefinitionKind::Implementation,
                position,
                cx,
            ),
            LspPickerKind::TypeDefinition => {
                editor.definition_locations_of_kind_at(GotoDefinitionKind::Type, position, cx)
            }
        }
    }
}

struct LspLocationsPicker {
    picker: Entity<Picker<LspLocationsDelegate>>,
    _subscription: Subscription,
}

impl LspLocationsPicker {
    fn open_for_editor(
        kind: LspPickerKind,
        editor: WeakEntity<Editor>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(editor) = editor.upgrade() else {
            return false;
        };
        let Some(workspace) = editor.read(cx).workspace() else {
            return false;
        };
        workspace.update(cx, |workspace, cx| {
            if source_editor(&editor.downgrade(), workspace, cx).is_none() {
                return false;
            }
            Self::open(kind, editor, workspace, window, cx);
            true
        })
    }

    /// Opens the picker for `kind`: runs a fresh LSP query and shows the
    /// results. An empty definitions query falls back to references when the
    /// `go_to_definition_fallback` setting calls for it, matching
    /// [`Editor::go_to_definition`].
    fn open(
        kind: LspPickerKind,
        editor: Entity<Editor>,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let (position, request) = editor.update(cx, |editor, _| {
            (
                editor.selections.newest_anchor().head(),
                editor.begin_navigation(),
            )
        });
        Self::open_with_request(
            kind,
            editor.downgrade(),
            position,
            None,
            request,
            workspace,
            window,
            cx,
        );
    }

    fn open_with_request(
        kind: LspPickerKind,
        editor: WeakEntity<Editor>,
        position: editor::Anchor,
        origin: Option<NavigationEntry>,
        request: NavigationRequest,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let Some(source) = editor.upgrade() else {
            return;
        };
        let project = workspace.project().clone();
        let fallback = EditorSettings::get_global(cx).go_to_definition_fallback;
        let task = cx.spawn_in(window, async move |workspace, cx| {
            // The kind the user invoked, kept for user-facing messages even if
            // the query below falls back to references.
            let invoked_kind = kind;
            let mut kind = kind;

            // Count on the built matches (not raw locations): they are deduped by
            // range and exclude fileless results, so a single distinct result
            // jumps directly and a fileless-only result reports "no results"
            // instead of opening a blank picker.
            let Some(mut matches) =
                run_picker_matches(kind, &editor, &workspace, &project, position, &request, cx)
                    .await
            else {
                return Ok(());
            };

            if matches.is_empty()
                && kind == LspPickerKind::Definition
                && fallback == GoToDefinitionFallback::FindAllReferences
            {
                kind = LspPickerKind::References;
                let Some(references) =
                    run_picker_matches(kind, &editor, &workspace, &project, position, &request, cx)
                        .await
                else {
                    return Ok(());
                };
                matches = references;
            }

            let navigation = workspace
                .update_in(cx, |workspace, window, cx| {
                    let source = source_editor(&editor, workspace, cx)?;
                    if !request.is_current(source.read(cx)) {
                        return None;
                    }
                    if matches.is_empty() {
                        struct NoLspResults;
                        workspace.show_toast(
                            Toast::new(
                                NotificationId::unique::<NoLspResults>(),
                                invoked_kind.empty_message(),
                            )
                            .autohide(),
                            cx,
                        );
                    } else if matches.len() == 1 {
                        let location_match = matches.pop()?;
                        return Some(source.update(cx, |editor, cx| {
                            open_location(
                                editor,
                                Location {
                                    buffer: location_match.buffer,
                                    range: location_match.anchor_range,
                                },
                                origin,
                                false,
                                window,
                                cx,
                            )
                        }));
                    } else {
                        Self::present(kind, matches, editor, origin, workspace, window, cx);
                    }
                    None
                })
                .ok()
                .flatten();
            if let Some(navigation) = navigation {
                navigation.await?;
            }
            Ok(())
        });
        source.update(cx, |editor, cx| editor.run_navigation_task(task, cx));
    }

    fn present(
        kind: LspPickerKind,
        matches: Vec<LocationMatch>,
        editor: WeakEntity<Editor>,
        origin: Option<NavigationEntry>,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        if workspace.active_modal::<Self>(cx).is_some() && !workspace.hide_modal(window, cx) {
            return;
        }
        let delegate = LspLocationsDelegate::new(
            kind,
            matches,
            workspace.project().clone(),
            cx.weak_entity(),
            editor,
            origin,
        );
        workspace.toggle_modal(window, cx, |window, cx| Self::new(delegate, window, cx));
    }

    fn new(delegate: LspLocationsDelegate, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let preview = picker_preview::editor_preview(delegate.project.clone(), window, cx);
        let picker = cx.new(|cx| Picker::list_with_preview(delegate, preview, window, cx));
        let subscription = cx.subscribe(&picker, |_, _, _: &DismissEvent, cx| {
            cx.emit(DismissEvent);
        });
        Self {
            picker,
            _subscription: subscription,
        }
    }
}

impl ModalView for LspLocationsPicker {}

impl EventEmitter<DismissEvent> for LspLocationsPicker {}

impl Focusable for LspLocationsPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for LspLocationsPicker {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().child(self.picker.clone())
    }
}

struct LocationMatch {
    path: ProjectPath,
    buffer: Entity<Buffer>,
    anchor_range: Range<Anchor>,
    range: Range<usize>,
    display_text: String,
    syntax_highlights: Vec<(Range<usize>, HighlightId)>,
    match_range: Range<usize>,
    line_number: u32,
}

/// A row in the grouped display list: a non-selectable file header, a match, or
/// a separator between file groups. `selected_index` indexes into this list.
enum Entry {
    Header(ProjectPath),
    Match(usize),
    Separator,
}

struct LspLocationsDelegate {
    kind: LspPickerKind,
    project: Entity<Project>,
    workspace: WeakEntity<Workspace>,
    editor: WeakEntity<Editor>,
    origin: Option<NavigationEntry>,
    all_matches: Vec<LocationMatch>,
    candidates: Arc<[StringMatchCandidate]>,
    matches: Vec<usize>,
    entries: Vec<Entry>,
    selected_index: usize,
    max_line_number: u32,
}

impl LspLocationsDelegate {
    fn new(
        kind: LspPickerKind,
        all_matches: Vec<LocationMatch>,
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        editor: WeakEntity<Editor>,
        origin: Option<NavigationEntry>,
    ) -> Self {
        // Match against the line text and the file path, mirroring the fuzzy
        // matching every other Zed picker uses.
        let candidates = all_matches
            .iter()
            .enumerate()
            .map(|(index, location_match)| {
                StringMatchCandidate::new(
                    index,
                    &format!(
                        "{} {}",
                        location_match.display_text,
                        location_match.path.path.as_unix_str()
                    ),
                )
            })
            .collect();
        let matches = (0..all_matches.len()).collect();
        let mut delegate = Self {
            kind,
            project,
            workspace,
            editor,
            origin,
            all_matches,
            candidates,
            matches,
            entries: Vec::new(),
            selected_index: 0,
            max_line_number: 0,
        };
        delegate.rebuild_entries();
        delegate
    }

    /// Rebuilds the grouped [`Self::entries`] from the filtered [`Self::matches`]:
    /// one header per file, its matches, and a separator before every group
    /// after the first. Selection snaps to the first selectable row.
    fn rebuild_entries(&mut self) {
        let mut entries = Vec::with_capacity(self.matches.len());
        let mut last_path: Option<&ProjectPath> = None;
        let mut max_line_number = 0;
        for &match_index in &self.matches {
            let location_match = &self.all_matches[match_index];
            if last_path != Some(&location_match.path) {
                if last_path.is_some() {
                    entries.push(Entry::Separator);
                }
                entries.push(Entry::Header(location_match.path.clone()));
                last_path = Some(&location_match.path);
            }
            max_line_number = max_line_number.max(location_match.line_number);
            entries.push(Entry::Match(match_index));
        }
        self.entries = entries;
        self.max_line_number = max_line_number;
        self.selected_index = self.first_selectable_index().unwrap_or(0);
    }

    fn first_selectable_index(&self) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| matches!(entry, Entry::Match(_)))
    }

    fn selected_location_match(&self) -> Option<&LocationMatch> {
        match self.entries.get(self.selected_index)? {
            Entry::Match(match_index) => self.all_matches.get(*match_index),
            Entry::Header(_) | Entry::Separator => None,
        }
    }

    fn open_selected(&mut self, split: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(location_match) = self.selected_location_match() else {
            return;
        };
        let location = Location {
            buffer: location_match.buffer.clone(),
            range: location_match.anchor_range.clone(),
        };
        let editor = self
            .workspace
            .upgrade()
            .and_then(|workspace| source_editor(&self.editor, workspace.read(cx), cx));
        let Some(editor) = editor else {
            cx.emit(DismissEvent);
            return;
        };
        editor.update(cx, |editor, cx| {
            editor.begin_navigation();
            let task = open_location(editor, location, self.origin.clone(), split, window, cx);
            editor.run_navigation_task(task, cx);
        });
        cx.emit(DismissEvent);
    }
}

fn build_location_matches(locations: &[Location], cx: &App) -> Vec<LocationMatch> {
    use gpui::EntityId;
    let mut snapshots: HashMap<EntityId, language::BufferSnapshot> = HashMap::default();
    let mut matches = Vec::with_capacity(locations.len());

    for location in locations {
        let snapshot = snapshots
            .entry(location.buffer.entity_id())
            .or_insert_with(|| location.buffer.read(cx).snapshot());

        let Some(file) = snapshot.file() else {
            continue;
        };
        let path = ProjectPath {
            worktree_id: file.worktree_id(cx),
            path: file.path().clone(),
        };

        let start_offset: usize = snapshot.summary_for_anchor(&location.range.start);
        let end_offset: usize = snapshot.summary_for_anchor(&location.range.end);
        let row = snapshot.offset_to_point(start_offset).row;
        let line_start = snapshot.point_to_offset(Point::new(row, 0));
        let line_end = snapshot.point_to_offset(Point::new(row, snapshot.line_len(row)));
        let full_line: String = snapshot.text_for_range(line_start..line_end).collect();

        // The row shows the line with leading indentation trimmed. Offsets below
        // are relative to that displayed text.
        let display_text = full_line.trim_start().to_string();
        let visible_start = line_end.saturating_sub(display_text.len());
        let visible_end = line_end;

        // Precompute syntax highlights for the displayed text so rendering a row
        // never re-snapshots the buffer or re-runs highlighting.
        let mut syntax_highlights = Vec::new();
        let mut offset = 0;
        for chunk in snapshot.chunks(
            visible_start..visible_end,
            LanguageAwareStyling {
                tree_sitter: true,
                diagnostics: false,
            },
        ) {
            let chunk_len = chunk.text.len();
            if let Some(id) = chunk.syntax_highlight_id {
                syntax_highlights.push((offset..offset + chunk_len, id));
            }
            offset += chunk_len;
        }

        // The match span, clamped into the displayed text. `clamp` bounds each
        // endpoint to the line; `min`/`max` then keep the range well-ordered even
        // for a malformed/inverted LSP range (clamping alone preserves bounds but
        // not `start <= end`).
        let clamped_start = start_offset.clamp(visible_start, visible_end) - visible_start;
        let clamped_end = end_offset.clamp(visible_start, visible_end) - visible_start;
        let match_range = clamped_start.min(clamped_end)..clamped_start.max(clamped_end);

        matches.push(LocationMatch {
            path,
            buffer: location.buffer.clone(),
            anchor_range: location.range.clone(),
            range: start_offset..end_offset,
            display_text,
            syntax_highlights,
            match_range,
            line_number: row + 1,
        });
    }

    // Group by file and order by position so the grouped display list is stable,
    // then drop exact-duplicate ranges a server may report more than once.
    matches.sort_by(|a, b| a.path.cmp(&b.path).then(a.range.start.cmp(&b.range.start)));
    matches.dedup_by(|a, b| a.path == b.path && a.range == b.range);
    matches
}

impl PickerDelegate for LspLocationsDelegate {
    type ListItem = AnyElement;

    fn name() -> &'static str {
        "lsp locations picker"
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> std::sync::Arc<str> {
        self.kind.placeholder().into()
    }

    fn match_count(&self) -> usize {
        self.entries.len()
    }

    fn can_select(&self, ix: usize, _window: &mut Window, _cx: &mut Context<Picker<Self>>) -> bool {
        matches!(self.entries.get(ix), Some(Entry::Match(_)))
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn select_on_hover(&self) -> bool {
        false
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let query = query.trim().to_owned();
        let candidates = self.candidates.clone();
        cx.spawn(async move |picker, cx| {
            let matches = if query.is_empty() {
                (0..candidates.len()).collect()
            } else {
                let string_matches = fuzzy::match_strings(
                    &candidates,
                    &query,
                    false,
                    true,
                    candidates.len(),
                    &Default::default(),
                    cx.background_executor().clone(),
                )
                .await;
                let mut indices = string_matches
                    .into_iter()
                    .map(|string_match| string_match.candidate_id)
                    .collect::<Vec<_>>();
                // Restore the file-grouped, positional order (fuzzy returns by score).
                indices.sort_unstable();
                indices
            };
            picker
                .update(cx, |picker, cx| {
                    picker.delegate.matches = matches;
                    picker.delegate.rebuild_entries();
                    cx.notify();
                })
                .ok();
        })
    }

    fn confirm(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.open_selected(secondary, window, cx);
    }

    fn dismissed(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        cx.emit(DismissEvent);
    }

    fn try_get_preview_data_for_match(&self, _cx: &App) -> Option<picker::PreviewUpdate> {
        let location_match = self.selected_location_match()?;
        Some(picker::PreviewUpdate::from_buffer(
            location_match.buffer.clone(),
            picker::MatchLocation {
                anchor_range: location_match.anchor_range.clone(),
                range: location_match.range.clone(),
            },
        ))
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        match self.entries.get(ix)? {
            Entry::Separator => Some(
                div()
                    .py(DynamicSpacing::Base04.rems(cx))
                    .child(Divider::horizontal())
                    .into_any_element(),
            ),
            Entry::Header(path) => {
                let path_style = self.project.read(cx).path_style(cx);
                let file_name = path
                    .path
                    .file_name()
                    .map(|name| name.to_string())
                    .unwrap_or_default();
                let directory = path
                    .path
                    .parent()
                    .map(|parent| parent.display(path_style))
                    .map(SharedString::new)
                    .unwrap_or_default();
                let file_icon = ItemSettings::get_global(cx)
                    .file_icons
                    .then(|| FileIcons::get_icon(path.path.as_std_path(), cx))
                    .flatten()
                    .map(|icon| {
                        Icon::from_path(icon)
                            .color(Color::Muted)
                            .size(IconSize::Small)
                    });
                Some(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .px(DynamicSpacing::Base06.rems(cx))
                        .py_1()
                        .gap_1p5()
                        .children(file_icon)
                        .child(
                            h_flex()
                                .gap_1()
                                .child(Label::new(file_name).size(LabelSize::Small))
                                .when(!directory.is_empty(), |container| {
                                    container.child(
                                        Label::new(directory)
                                            .size(LabelSize::Small)
                                            .color(Color::Muted)
                                            .truncate_start(),
                                    )
                                }),
                        )
                        .into_any_element(),
                )
            }
            Entry::Match(match_index) => {
                let location_match = self.all_matches.get(*match_index)?;
                Some(
                    ListItem::new(ix)
                        .spacing(ListItemSpacing::Sparse)
                        .inset(true)
                        .toggle_state(selected)
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_2p5()
                                .text_sm()
                                .child(
                                    h_flex()
                                        .w(rems(
                                            (self.max_line_number.max(1).ilog10() + 1) as f32 * 0.5,
                                        ))
                                        .justify_end()
                                        .child(
                                            Label::new(location_match.line_number.to_string())
                                                .color(Color::Custom(
                                                    cx.theme().colors().text_muted.opacity(0.5),
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(render_matched_line(location_match, cx)),
                                ),
                        )
                        .into_any_element(),
                )
            }
        }
    }
}

/// Renders the precomputed displayed line, resolving the stored syntax highlight
/// ids against the current theme and overlaying the match with a highlighted
/// background and bold weight.
fn render_matched_line(location_match: &LocationMatch, cx: &App) -> StyledText {
    let settings = ThemeSettings::get_global(cx);
    let text_style = TextStyle {
        color: cx.theme().colors().text,
        font_family: settings.buffer_font.family.clone(),
        font_features: settings.buffer_font.features.clone(),
        font_fallbacks: settings.buffer_font.fallbacks.clone(),
        font_size: settings.buffer_font_size(cx).into(),
        font_weight: settings.buffer_font.weight,
        line_height: relative(1.),
        ..Default::default()
    };

    let syntax_theme = cx.theme().syntax();
    let syntax_highlights = location_match
        .syntax_highlights
        .iter()
        .filter_map(|(range, id)| Some((range.clone(), syntax_theme.get(*id).copied()?)))
        .collect::<Vec<_>>();

    let match_style = HighlightStyle {
        background_color: Some(cx.theme().colors().search_match_background),
        font_weight: Some(gpui::FontWeight::BOLD),
        ..Default::default()
    };
    let match_highlight = (location_match.match_range.clone(), match_style);

    let highlights = gpui::combine_highlights(syntax_highlights, [match_highlight]);
    StyledText::new(location_match.display_text.clone())
        .with_default_highlights(&text_style, highlights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor::hover_links::{DefinitionLocations, NavigationSource};
    use editor::test::editor_lsp_test_context::EditorLspTestContext;
    use editor::{EditorMode, HighlightKey};
    use gpui::{Modifiers, TestAppContext};
    use indoc::indoc;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;
    use workspace::Item as _;
    use workspace::pane::{GoToNewerTag, GoToOlderTag};

    gpui::actions!(lsp_locations_test, [TestPickerNavigation]);

    const SOURCE: &str = indoc! {r#"
        fn main() {
            let aˇbc = 123;
            let xyz = abc;
        }
    "#};

    const TARGETS: &[(u32, u32, u32)] = &[(0, 3, 7), (2, 14, 17)];
    const LATEST_TARGETS: &[(u32, u32, u32)] = &[(1, 8, 11), (1, 14, 17)];
    const SECOND_SOURCE: &str = indoc! {r#"
        fn main() {
            let abc = 123;
            let xyz = aˇbc;
        }
    "#};
    const MODES: [Option<EditorMode>; 3] = [
        None,
        Some(EditorMode::SingleLine),
        Some(EditorMode::AutoHeight {
            min_lines: 1,
            max_lines: Some(4),
        }),
    ];
    const KINDS: [LspPickerKind; 5] = [
        LspPickerKind::Definition,
        LspPickerKind::Declaration,
        LspPickerKind::Implementation,
        LspPickerKind::TypeDefinition,
        LspPickerKind::References,
    ];

    #[gpui::test]
    async fn test_multiple_references_open_picker(cx: &mut TestAppContext) {
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                references_provider: Some(lsp::OneOf::Left(true)),
                ..Default::default()
            },
            cx,
        )
        .await;
        cx.set_state(SOURCE);
        cx.lsp
            .set_request_handler::<lsp::request::References, _, _>(async move |params, _| {
                let uri = params.text_document_position.text_document.uri;
                Ok(Some(references(uri, &[(1, 8, 11), (2, 14, 17)])))
            });

        open(&mut cx, LspPickerKind::References);

        assert!(
            active_picker(&mut cx).is_some(),
            "multiple references should open the picker"
        );
    }

    #[gpui::test]
    async fn test_single_result_jumps_without_picker(cx: &mut TestAppContext) {
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                references_provider: Some(lsp::OneOf::Left(true)),
                ..Default::default()
            },
            cx,
        )
        .await;
        cx.set_state(SOURCE);
        cx.lsp
            .set_request_handler::<lsp::request::References, _, _>(async move |params, _| {
                let uri = params.text_document_position.text_document.uri;
                Ok(Some(references(uri, &[(2, 14, 17)])))
            });

        open(&mut cx, LspPickerKind::References);

        assert!(
            active_picker(&mut cx).is_none(),
            "a single result should jump directly instead of opening the picker"
        );
        // The lone result at row 2 should be selected directly, moving the
        // cursor off its starting position on row 1.
        cx.assert_editor_state(indoc! {r#"
            fn main() {
                let abc = 123;
                let xyz = «abcˇ»;
            }
        "#});
    }

    #[gpui::test]
    async fn test_no_results_does_not_open_picker(cx: &mut TestAppContext) {
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                references_provider: Some(lsp::OneOf::Left(true)),
                ..Default::default()
            },
            cx,
        )
        .await;
        cx.set_state(SOURCE);
        cx.lsp
            .set_request_handler::<lsp::request::References, _, _>(async move |_params, _| {
                Ok(Some(Vec::new()))
            });

        open(&mut cx, LspPickerKind::References);

        assert!(
            active_picker(&mut cx).is_none(),
            "an empty result should not open the picker"
        );
    }

    #[gpui::test]
    async fn test_definition_falls_back_to_references_picker(cx: &mut TestAppContext) {
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                definition_provider: Some(lsp::OneOf::Left(true)),
                references_provider: Some(lsp::OneOf::Left(true)),
                ..Default::default()
            },
            cx,
        )
        .await;
        cx.set_state(SOURCE);
        cx.lsp
            .set_request_handler::<lsp::request::GotoDefinition, _, _>(async move |_params, _| {
                Ok(None)
            });
        cx.lsp
            .set_request_handler::<lsp::request::References, _, _>(async move |params, _| {
                let uri = params.text_document_position.text_document.uri;
                Ok(Some(references(uri, &[(1, 8, 11), (2, 14, 17)])))
            });

        open(&mut cx, LspPickerKind::Definition);

        assert!(
            active_picker(&mut cx).is_some(),
            "an empty definition query should fall back to the references picker"
        );
    }

    #[gpui::test]
    async fn test_fuzzy_filter_matches_subsequence(cx: &mut TestAppContext) {
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                references_provider: Some(lsp::OneOf::Left(true)),
                ..Default::default()
            },
            cx,
        )
        .await;
        cx.set_state(SOURCE);
        cx.lsp
            .set_request_handler::<lsp::request::References, _, _>(async move |params, _| {
                let uri = params.text_document_position.text_document.uri;
                Ok(Some(references(uri, &[(1, 8, 11), (2, 14, 17)])))
            });

        open(&mut cx, LspPickerKind::References);
        let modal = active_picker(&mut cx).expect("multiple references should open the picker");
        let picker = cx.update(|_window, cx| modal.read(cx).picker.clone());

        let matches = |cx: &mut EditorLspTestContext, query: &str| -> usize {
            cx.update(|window, cx| {
                picker.update(cx, |picker, cx| picker.set_query(query, window, cx));
            });
            cx.run_until_parked();
            cx.update(|_window, cx| picker.read(cx).delegate.matches.len())
        };

        // "lx" is a subsequence of "let xyz" but not a substring of either line,
        // so it only matches with fuzzy matching.
        assert_eq!(matches(&mut cx, "lx"), 1);
        assert_eq!(matches(&mut cx, "zzzz"), 0);
        assert_eq!(matches(&mut cx, ""), 2);
    }

    #[gpui::test]
    async fn test_cmd_click_fallback_honors_lsp_results_location(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let definitions = track_definition_requests(&mut cx, GotoDefinitionKind::Symbol, &[]);
        let references = track_reference_requests(&mut cx);
        click(&mut cx, SOURCE, Modifiers::secondary_key());
        assert_requests(&definitions, &[(1, 9)]);
        assert_requests(&references, &[(1, 9)]);
        assert_picker_locations(
            &mut cx,
            LspPickerKind::References,
            &[(1, 8, 11), (2, 14, 17)],
        );
        cx.assert_editor_state(SOURCE);
        assert_hover_cleared(&mut cx);
        assert_no_picker_notifications(&mut cx);
    }

    #[gpui::test]
    async fn test_type_definition_honors_lsp_results_location(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                type_definition_provider: Some(lsp::TypeDefinitionProviderCapability::Simple(true)),
                ..Default::default()
            },
            cx,
        )
        .await;
        cx.update(|_window, cx| {
            cx.update_global::<settings::SettingsStore, _>(|settings, cx| {
                settings.update_user_settings(cx, |settings| {
                    settings.editor.lsp_results_location = Some(OpenResultsIn::Picker);
                });
            });
        });
        cx.set_state(indoc! {r#"
            fn main() {
                struct Foo<T>(T);
                struct Bar;
                let fˇoo: Foo<Bar>;
            }
        "#});
        cx.lsp
            .set_request_handler::<lsp::request::GotoTypeDefinition, _, _>(
                async move |params, _| {
                    let uri = params.text_document_position_params.text_document.uri;
                    Ok(Some(lsp::GotoDefinitionResponse::Array(references(
                        uri,
                        &[(1, 11, 14), (2, 11, 14)],
                    ))))
                },
            );

        cx.dispatch_action(GoToTypeDefinition::default());

        assert!(
            active_picker(&mut cx).is_some(),
            "type definition should open the picker when lsp_results_location is picker"
        );
    }

    #[gpui::test]
    async fn test_declaration_honors_lsp_results_location(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                declaration_provider: Some(lsp::DeclarationCapability::Simple(true)),
                ..Default::default()
            },
            cx,
        )
        .await;
        cx.update(|_window, cx| {
            cx.update_global::<settings::SettingsStore, _>(|settings, cx| {
                settings.update_user_settings(cx, |settings| {
                    settings.editor.lsp_results_location = Some(OpenResultsIn::Picker);
                });
            });
        });
        cx.set_state(indoc! {r#"
            fn main() {
                let foo = ();
                let foo = ();
                let bar = fˇoo;
            }
        "#});
        cx.lsp
            .set_request_handler::<lsp::request::GotoDeclaration, _, _>(async move |params, _| {
                let uri = params.text_document_position_params.text_document.uri;
                Ok(Some(lsp::GotoDefinitionResponse::Array(references(
                    uri,
                    &[(1, 8, 11), (2, 8, 11)],
                ))))
            });

        cx.dispatch_action(GoToDeclaration::default());

        assert!(
            active_picker(&mut cx).is_some(),
            "declaration should open the picker when lsp_results_location is picker"
        );
    }

    #[gpui::test]
    async fn test_cmd_click_multiple_definitions_honors_lsp_results_location(
        cx: &mut TestAppContext,
    ) {
        assert_cached_definition_picker(GotoDefinitionKind::Symbol, cx).await;
    }

    #[gpui::test]
    async fn test_cmd_shift_click_type_definitions_honor_lsp_results_location(
        cx: &mut TestAppContext,
    ) {
        assert_cached_definition_picker(GotoDefinitionKind::Type, cx).await;
    }

    #[gpui::test]
    async fn test_cmd_click_no_definition_respects_disabled_fallback(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                definition_provider: Some(lsp::OneOf::Left(true)),
                references_provider: Some(lsp::OneOf::Left(true)),
                ..Default::default()
            },
            cx,
        )
        .await;
        set_picker(&mut cx);
        cx.update(|_window, cx| {
            cx.update_global::<settings::SettingsStore, _>(|settings, cx| {
                settings.update_user_settings(cx, |settings| {
                    settings.editor.go_to_definition_fallback = Some(GoToDefinitionFallback::None);
                });
            });
        });
        cx.set_state(SOURCE);
        cx.lsp
            .set_request_handler::<lsp::request::GotoDefinition, _, _>(async move |_params, _| {
                Ok(None)
            });
        cx.lsp
            .set_request_handler::<lsp::request::References, _, _>(async move |_params, _| {
                panic!(
                    "cmd-click should not request references when go_to_definition_fallback is none"
                )
            });

        let screen_coord = cx
            .editor(|editor, _, cx| editor.pixel_position_of_cursor(cx))
            .unwrap();
        cx.simulate_click(screen_coord, gpui::Modifiers::secondary_key());
        cx.run_until_parked();

        assert!(
            active_picker(&mut cx).is_none(),
            "cmd-click should not open the references picker when go_to_definition_fallback is none"
        );
    }

    #[gpui::test]
    async fn test_cmd_click_cached_single_definition_preserves_native_navigation(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        for kind in [GotoDefinitionKind::Symbol, GotoDefinitionKind::Type] {
            let mut cx = definition_cx(cx).await;
            let requests =
                track_definition_requests(&mut cx, kind, &[(2, 14, 17), (1, 8, 11), (2, 14, 17)]);
            let position = cx.pixel_position(SOURCE);
            let modifiers = definition_modifiers(kind);
            cx.simulate_mouse_move(position, None, modifiers);
            cx.run_until_parked();
            assert_definition_highlight(&mut cx);
            assert_eq!(
                *requests.lock().expect("requests"),
                [lsp::Position::new(1, 9)]
            );

            cx.simulate_click(position, modifiers);
            cx.run_until_parked();

            assert!(active_picker(&mut cx).is_none());
            cx.assert_editor_state(indoc! {r#"
                fn main() {
                    let abc = 123;
                    let xyz = «abcˇ»;
                }
            "#});
            assert_hover_cleared(&mut cx);
            assert_eq!(
                *requests.lock().expect("requests"),
                [lsp::Position::new(1, 9)]
            );
        }
    }

    #[gpui::test]
    async fn test_cmd_click_cold_definition_uses_click_position(cx: &mut TestAppContext) {
        cx.update(crate::init);
        for kind in [GotoDefinitionKind::Symbol, GotoDefinitionKind::Type] {
            let mut cx = definition_cx(cx).await;
            let requests =
                track_definition_requests(&mut cx, kind, &[(2, 14, 17), (1, 8, 11), (0, 3, 7)]);
            let position = cx.pixel_position(indoc! {r#"
                fn main() {
                    let abc = 123;
                    let xyz = aˇbc;
                }
            "#});
            cx.assert_editor_state(SOURCE);
            assert_hover_cleared(&mut cx);

            cx.simulate_click(position, definition_modifiers(kind));
            cx.run_until_parked();

            assert_eq!(
                *requests.lock().expect("requests"),
                [lsp::Position::new(2, 15)]
            );
            assert_picker_locations(
                &mut cx,
                if kind == GotoDefinitionKind::Type {
                    LspPickerKind::TypeDefinition
                } else {
                    LspPickerKind::Definition
                },
                &[(0, 3, 7), (1, 8, 11)],
            );
            assert_hover_cleared(&mut cx);
        }
    }

    #[gpui::test]
    async fn test_cmd_shift_click_empty_types_respects_references_fallback(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        for fallback in [
            GoToDefinitionFallback::FindAllReferences,
            GoToDefinitionFallback::None,
        ] {
            let mut cx = definition_cx(cx).await;
            cx.update(|_, cx| {
                cx.update_global::<settings::SettingsStore, _>(|settings, cx| {
                    settings.update_user_settings(cx, |settings| {
                        settings.editor.go_to_definition_fallback = Some(fallback);
                    });
                });
            });
            let definitions = track_definition_requests(&mut cx, GotoDefinitionKind::Type, &[]);
            let references = Arc::new(Mutex::new(Vec::new()));
            cx.lsp
                .set_request_handler::<lsp::request::References, _, _>({
                    let references = references.clone();
                    let uri = cx.buffer_lsp_url.clone();
                    move |params, _| {
                        assert_eq!(params.text_document_position.text_document.uri, uri);
                        references
                            .lock()
                            .expect("requests")
                            .push(params.text_document_position.position);
                        let result = self::references(uri.clone(), &[(1, 8, 11), (2, 14, 17)]);
                        async move { Ok(Some(result)) }
                    }
                });
            let position = cx.pixel_position(SOURCE);

            cx.simulate_click(position, definition_modifiers(GotoDefinitionKind::Type));
            cx.run_until_parked();

            assert_eq!(
                *definitions.lock().expect("requests"),
                [lsp::Position::new(1, 9)]
            );
            if fallback == GoToDefinitionFallback::FindAllReferences {
                assert_eq!(
                    *references.lock().expect("requests"),
                    [lsp::Position::new(1, 9)]
                );
                assert_picker_locations(
                    &mut cx,
                    LspPickerKind::References,
                    &[(1, 8, 11), (2, 14, 17)],
                );
            } else {
                assert_eq!(
                    *references.lock().expect("requests"),
                    Vec::<lsp::Position>::new()
                );
                assert!(active_picker(&mut cx).is_none());
            }
            cx.assert_editor_state(SOURCE);
            assert_hover_cleared(&mut cx);
        }
    }

    #[gpui::test]
    async fn test_cmd_shift_click_during_type_hover_ignores_cached_symbol(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let symbols = track_definition_requests(&mut cx, GotoDefinitionKind::Symbol, &[(0, 3, 7)]);
        let types = Arc::new(Mutex::new(Vec::new()));
        cx.lsp
            .set_request_handler::<lsp::request::GotoTypeDefinition, _, _>({
                let types = types.clone();
                let uri = cx.buffer_lsp_url.clone();
                move |params, cx| {
                    assert_eq!(params.text_document_position_params.text_document.uri, uri);
                    let request_count = {
                        let mut types = types.lock().expect("requests");
                        types.push(params.text_document_position_params.position);
                        types.len()
                    };
                    let result = definitions(uri.clone(), &[(2, 8, 11), (2, 14, 17)]);
                    async move {
                        if request_count == 1 {
                            cx.background_executor().timer(Duration::from_secs(1)).await;
                        }
                        Ok(Some(result))
                    }
                }
            });
        let position = cx.pixel_position(SOURCE);
        cx.simulate_mouse_move(position, None, Modifiers::secondary_key());
        cx.run_until_parked();
        assert_definition_highlight(&mut cx);
        assert_eq!(
            *symbols.lock().expect("requests"),
            [lsp::Position::new(1, 9)]
        );
        let modifiers = definition_modifiers(GotoDefinitionKind::Type);
        cx.simulate_modifiers_change(modifiers);
        cx.run_until_parked();
        assert_eq!(*types.lock().expect("requests"), [lsp::Position::new(1, 9)]);
        assert!(active_picker(&mut cx).is_none());

        cx.simulate_click(position, modifiers);
        cx.run_until_parked();

        assert_picker_locations(
            &mut cx,
            LspPickerKind::TypeDefinition,
            &[(2, 8, 11), (2, 14, 17)],
        );
        assert_hover_cleared(&mut cx);
        assert_eq!(
            *symbols.lock().expect("requests"),
            [lsp::Position::new(1, 9)]
        );
        assert_eq!(
            *types.lock().expect("requests"),
            [lsp::Position::new(1, 9), lsp::Position::new(1, 9)],
        );
        cx.background_executor.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert_picker_locations(
            &mut cx,
            LspPickerKind::TypeDefinition,
            &[(2, 8, 11), (2, 14, 17)],
        );
        assert_hover_cleared(&mut cx);
    }

    #[gpui::test]
    async fn test_cmd_click_during_new_symbol_hover_ignores_cached_symbol(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        cx.lsp
            .set_request_handler::<lsp::request::GotoDefinition, _, _>({
                let requests = requests.clone();
                let uri = cx.buffer_lsp_url.clone();
                move |params, cx| {
                    assert_eq!(params.text_document_position_params.text_document.uri, uri);
                    let position = params.text_document_position_params.position;
                    let request_count = {
                        let mut requests = requests.lock().expect("requests");
                        requests.push(position);
                        requests.len()
                    };
                    let result = match (position.line, position.character) {
                        (1, 9) => definitions(uri.clone(), &[(0, 3, 7)]),
                        (2, 9) => definitions(uri.clone(), &[(1, 8, 11), (2, 14, 17)]),
                        _ => panic!("unexpected definition position: {position:?}"),
                    };
                    async move {
                        if request_count == 2 {
                            cx.background_executor().timer(Duration::from_secs(1)).await;
                        }
                        Ok(Some(result))
                    }
                }
            });
        let first_position = cx.pixel_position(SOURCE);
        let second_position = cx.pixel_position(indoc! {r#"
            fn main() {
                let abc = 123;
                let xˇyz = abc;
            }
        "#});
        let modifiers = Modifiers::secondary_key();
        cx.simulate_mouse_move(first_position, None, modifiers);
        cx.run_until_parked();
        assert_definition_highlight(&mut cx);
        assert_eq!(
            *requests.lock().expect("requests"),
            [lsp::Position::new(1, 9)]
        );

        cx.simulate_mouse_move(second_position, None, modifiers);
        cx.run_until_parked();
        assert_eq!(
            *requests.lock().expect("requests"),
            [lsp::Position::new(1, 9), lsp::Position::new(2, 9)],
        );
        cx.assert_editor_state(SOURCE);
        assert!(active_picker(&mut cx).is_none());

        cx.simulate_click(second_position, modifiers);
        cx.run_until_parked();

        assert_eq!(
            *requests.lock().expect("requests"),
            [
                lsp::Position::new(1, 9),
                lsp::Position::new(2, 9),
                lsp::Position::new(2, 9)
            ],
        );
        assert_picker_locations(
            &mut cx,
            LspPickerKind::Definition,
            &[(1, 8, 11), (2, 14, 17)],
        );
        assert_hover_cleared(&mut cx);
        cx.background_executor.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(
            *requests.lock().expect("requests"),
            [
                lsp::Position::new(1, 9),
                lsp::Position::new(2, 9),
                lsp::Position::new(2, 9)
            ],
        );
        assert_picker_locations(
            &mut cx,
            LspPickerKind::Definition,
            &[(1, 8, 11), (2, 14, 17)],
        );
        assert_hover_cleared(&mut cx);
    }

    #[gpui::test]
    async fn test_cmd_click_default_and_split_preserve_native_navigation(cx: &mut TestAppContext) {
        cx.update(crate::init);
        for (open_results_in, split) in [
            (OpenResultsIn::MultiBuffer, false),
            (OpenResultsIn::Picker, true),
        ] {
            for kind in [GotoDefinitionKind::Symbol, GotoDefinitionKind::Type] {
                let mut cx = definition_cx(cx).await;
                cx.update(|_, cx| {
                    cx.update_global::<settings::SettingsStore, _>(|settings, cx| {
                        settings.update_user_settings(cx, |settings| {
                            settings.editor.lsp_results_location = Some(open_results_in);
                        });
                    });
                });
                let requests = track_definition_requests(&mut cx, kind, &[(0, 3, 7), (2, 14, 17)]);
                let position = cx.pixel_position(SOURCE);
                let modifiers = Modifiers {
                    alt: split,
                    ..definition_modifiers(kind)
                };
                cx.simulate_mouse_move(position, None, modifiers);
                cx.run_until_parked();
                assert_definition_highlight(&mut cx);

                cx.simulate_click(position, modifiers);
                cx.run_until_parked();

                assert!(active_picker(&mut cx).is_none());
                assert_hover_cleared(&mut cx);
                assert_eq!(
                    *requests.lock().expect("requests"),
                    [lsp::Position::new(1, 9)]
                );
                let workspace = cx.workspace.clone();
                let source_editor = cx.editor.clone();
                cx.update(|_, cx| {
                    let workspace = workspace.read(cx);
                    assert_eq!(workspace.panes().len(), if split { 2 } else { 1 });
                    let editor = workspace
                        .active_item_as::<Editor>(cx)
                        .expect("active editor");
                    if split {
                        assert_ne!(editor, source_editor);
                    } else {
                        assert_eq!(editor, source_editor);
                    }
                    editor.update(cx, |editor, cx| {
                        assert_eq!(
                            editor
                                .selections
                                .all::<Point>(&editor.display_snapshot(cx))
                                .into_iter()
                                .map(|selection| selection.range())
                                .collect::<Vec<_>>(),
                            [
                                Point::new(0, 3)..Point::new(0, 7),
                                Point::new(2, 14)..Point::new(2, 17)
                            ],
                        );
                    });
                });
            }
        }
    }

    #[gpui::test]
    async fn test_picker_navigation_without_workspace_propagates(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let standalone = cx.update(|window, cx| {
            let buffer = cx.new(|cx| Buffer::local("abc", cx));
            cx.new(|cx| Editor::for_buffer(buffer, None, window, cx))
        });
        let propagated = Arc::new(AtomicUsize::new(0));
        cx.update_editor(|editor, _, cx| {
            let standalone = standalone.downgrade();
            editor
                .register_action(move |_: &TestPickerNavigation, window, cx| {
                    handle_nav_action(
                        Some(OpenResultsIn::Picker),
                        LspPickerKind::Definition,
                        &standalone,
                        window,
                        cx,
                    );
                })
                .detach();
            App::on_action::<TestPickerNavigation>(cx, {
                let propagated = propagated.clone();
                move |_, _| {
                    propagated.fetch_add(1, Ordering::SeqCst);
                }
            });
        });

        cx.dispatch_action(TestPickerNavigation);

        assert_eq!(propagated.load(Ordering::SeqCst), 1);
        assert!(active_picker(&mut cx).is_none());
    }

    #[gpui::test]
    async fn test_definition_locations_without_registration_use_native_navigation(
        cx: &mut TestAppContext,
    ) {
        for initialize_late in [false, true] {
            let mut cx = definition_cx(cx).await;
            let requests = track_definition_requests(&mut cx, GotoDefinitionKind::Symbol, TARGETS);
            let references = track_reference_requests(&mut cx);
            if initialize_late {
                let position = cx.pixel_position(SOURCE);
                cx.simulate_mouse_move(position, None, Modifiers::secondary_key());
                cx.run_until_parked();
                assert_definition_highlight(&mut cx);
                cx.update(|_, cx| super::init(cx));
                click(&mut cx, SOURCE, Modifiers::secondary_key());
            } else {
                let locations = source_definition_locations(&mut cx);
                dispatch_definition_locations(&mut cx, GotoDefinitionKind::Symbol, locations);
            }
            assert_requests(&requests, if initialize_late { &[(1, 9)] } else { &[] });
            assert_requests(&references, &[]);
            assert_native_definition_locations(&mut cx, TARGETS);
            assert_hover_cleared(&mut cx);
        }
    }

    #[gpui::test]
    async fn test_cmd_click_ignored_source_preserves_manual_definition_navigation(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let project = cx.update_workspace(|workspace, _, _| workspace.project().clone());
        let fs = cx.update(|_, cx| project.read(cx).fs().clone());
        fs.as_fake()
            .insert_file(
                EditorLspTestContext::root_path().join(".gitignore"),
                b"dir/file.rs\n".to_vec(),
            )
            .await;
        cx.run_until_parked();
        cx.update_workspace(|workspace, _, cx| workspace.worktree_scans_complete(cx))
            .await;
        cx.buffer(|buffer, cx| {
            let file = project::File::from_dyn(buffer.file()).expect("project file");
            let worktree = project
                .read(cx)
                .worktree_for_id(file.worktree_id(cx), cx)
                .expect("source worktree");
            let entry = worktree
                .read(cx)
                .entry_for_id(file.project_entry_id().expect("source entry id"))
                .expect("source entry");
            assert!(entry.is_ignored);
        });
        let requests = track_definition_requests(&mut cx, GotoDefinitionKind::Symbol, TARGETS);
        let position = cx.pixel_position(SOURCE);
        assert_hover_cleared(&mut cx);

        cx.simulate_click(position, definition_modifiers(GotoDefinitionKind::Symbol));
        cx.run_until_parked();

        assert_requests(&requests, &[(1, 9)]);
        assert_picker_locations(&mut cx, LspPickerKind::Definition, TARGETS);
        assert_hover_cleared(&mut cx);
    }

    #[gpui::test]
    async fn test_picker_actions_decline_ineligible_editor(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let requests = KINDS.map(|kind| track_query(&mut cx, kind, &[], 0));
        for mode in MODES {
            for kind in KINDS {
                for (location, supplied_locations) in [
                    (None, false),
                    (Some(OpenResultsIn::Picker), false),
                    (Some(OpenResultsIn::MultiBuffer), false),
                    (None, true),
                ] {
                    if supplied_locations && kind == LspPickerKind::References {
                        continue;
                    }
                    for after_dispatch in [false, true] {
                        reset_navigation_editor(&mut cx, SOURCE);
                        let action: Box<dyn gpui::Action> = if supplied_locations {
                            let locations = source_definition_locations(&mut cx);
                            Box::new(cx.update_editor(|editor, _, cx| {
                                definition_action(editor, definition_kind(kind), locations, cx)
                            }))
                        } else {
                            query_action(kind, location)
                        };
                        cx.update_editor(|editor, window, cx| {
                            if !after_dispatch {
                                disable_picker_lsp_data(editor, mode.clone());
                            }
                            window.dispatch_action(action, cx);
                            if after_dispatch {
                                disable_picker_lsp_data(editor, mode.clone());
                            }
                        });
                        cx.run_until_parked();
                        assert_no_requests(&requests);
                        assert_no_navigation(&mut cx);
                    }
                }
            }
        }
    }

    #[gpui::test]
    async fn test_cmd_click_cached_and_cold_definitions_decline_disabled_lsp_data(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        let source = "fn main() { let aˇbc = 123; let xyz = abc; }";
        let targets = &[(0, 3, 7), (0, 37, 40)];
        let mut cx = definition_cx(cx).await;
        cx.set_state(source);
        for ranges in [&[][..], &targets[..1], targets] {
            let warm = !ranges.is_empty();
            for mode in MODES {
                for (kind, other_kind) in [
                    (GotoDefinitionKind::Symbol, GotoDefinitionKind::Type),
                    (GotoDefinitionKind::Type, GotoDefinitionKind::Symbol),
                ] {
                    for (location, split) in [
                        (OpenResultsIn::Picker, false),
                        (OpenResultsIn::MultiBuffer, false),
                        (OpenResultsIn::Picker, true),
                    ] {
                        reset_navigation_editor(&mut cx, source);
                        set_results(&mut cx, location);
                        let other = track_definition_requests(&mut cx, other_kind, &[]);
                        let requests = track_definition_requests(&mut cx, kind, ranges);
                        let references = track_reference_requests(&mut cx);
                        let modifiers = Modifiers {
                            alt: split,
                            ..definition_modifiers(kind)
                        };
                        let position = cx.pixel_position(source);
                        if warm {
                            cx.simulate_mouse_move(position, None, modifiers);
                            cx.run_until_parked();
                            cx.assert_editor_text_highlights(
                                HighlightKey::HoveredLinkState,
                                &source.replace("aˇbc", "«abc»"),
                            );
                            assert_requests(&requests, &[(0, 17)]);
                        }
                        cx.update_editor(|editor, _, cx| {
                            disable_picker_lsp_data(editor, mode.clone());
                            editor.show_cursor(cx);
                        });
                        cx.update(|window, cx| {
                            window.refresh();
                            let _ = window.draw(cx);
                        });
                        click(&mut cx, source, modifiers);
                        assert_requests(&requests, if warm { &[(0, 17)] } else { &[] });
                        assert_no_requests(&[other, references]);
                        assert_no_navigation_at(&mut cx, source);
                    }
                }
            }
        }
    }

    #[gpui::test]
    async fn test_pending_picker_queries_ignore_disabled_or_superseded_results(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        for (newer_click, mode) in
            std::iter::once((true, None)).chain(MODES.map(|mode| (false, mode)))
        {
            for (kind, fallback) in KINDS
                .map(|kind| (kind, false))
                .into_iter()
                .chain([(LspPickerKind::References, true)])
            {
                let single = if newer_click {
                    &TARGETS[..1]
                } else {
                    &TARGETS[1..]
                };
                for ranges in [&[][..], single, TARGETS] {
                    reset_navigation_editor(&mut cx, SOURCE);
                    let definitions =
                        track_definition_requests(&mut cx, GotoDefinitionKind::Symbol, &[]);
                    let references = track_reference_requests(&mut cx);
                    let locations = self::references(cx.buffer_lsp_url.clone(), ranges);
                    let (requests, responses) =
                        track_query_response(&mut cx, kind, move |_| (1, Ok(locations.clone())));
                    let invoked = if fallback {
                        LspPickerKind::Definition
                    } else {
                        kind
                    };
                    dispatch_query(&mut cx, invoked, None);
                    assert_requests(&requests, &[(1, 9)]);
                    assert_no_navigation(&mut cx);
                    let click_requests = newer_click.then(|| {
                        let requests =
                            track_query(&mut cx, LspPickerKind::Definition, LATEST_TARGETS, 0);
                        click(&mut cx, SECOND_SOURCE, Modifiers::secondary_key());
                        requests
                    });
                    if !newer_click {
                        cx.update_editor(|editor, _, _| {
                            disable_picker_lsp_data(editor, mode.clone())
                        });
                    }
                    for completed in [false, true] {
                        if completed {
                            advance(&mut cx);
                        }
                        assert_requests(
                            &responses,
                            if completed && !newer_click {
                                &[(1, 9)]
                            } else {
                                &[]
                            },
                        );
                        assert_requests(&requests, &[(1, 9)]);
                        if fallback {
                            assert_requests(&definitions, &[(1, 9)]);
                        }
                        if kind != LspPickerKind::References {
                            assert_requests(&references, &[]);
                        }
                        if let Some(requests) = &click_requests {
                            assert_requests(requests, &[(2, 15)]);
                            assert_picker_locations(
                                &mut cx,
                                LspPickerKind::Definition,
                                LATEST_TARGETS,
                            );
                            cx.assert_editor_state(SECOND_SOURCE);
                            assert_hover_cleared(&mut cx);
                            assert_no_picker_notifications(&mut cx);
                        } else {
                            assert_no_navigation(&mut cx);
                        }
                    }
                }
            }
        }
    }

    #[gpui::test]
    async fn test_picker_confirmation_declines_ineligible_editor(cx: &mut TestAppContext) {
        cx.update(crate::init);
        for (mode, removed, dropped) in MODES
            .map(|mode| (mode, false, false))
            .into_iter()
            .chain([(None, true, false), (None, true, true)])
        {
            for split in [false, true] {
                let mut cx = definition_cx(cx).await;
                let requests =
                    track_definition_requests(&mut cx, GotoDefinitionKind::Symbol, TARGETS);
                open(&mut cx, LspPickerKind::Definition);
                assert_picker_locations(&mut cx, LspPickerKind::Definition, TARGETS);
                let modal = active_picker(&mut cx).expect("locations picker");
                let picker = cx.update(|_, cx| modal.read(cx).picker.clone());
                if dropped {
                    let source = cx.editor.downgrade();
                    reset_navigation_editor(&mut cx, SOURCE);
                    cx.dispatch_action(workspace::ReopenLastPicker);
                    source.assert_released();
                    assert_eq!(active_picker(&mut cx), Some(modal));
                } else if removed {
                    let source = cx.editor.entity_id();
                    cx.update_workspace(|workspace, window, cx| {
                        workspace.active_pane().update(cx, |pane, cx| {
                            pane.remove_item(source, false, false, window, cx);
                        });
                    });
                    cx.run_until_parked();
                } else {
                    cx.update_editor(|editor, _, _| {
                        disable_picker_lsp_data(editor, mode.clone());
                    });
                }

                cx.update(|window, cx| {
                    picker.update(cx, |picker, cx| {
                        picker.delegate.confirm(split, window, cx);
                    });
                });
                cx.run_until_parked();

                assert_requests(&requests, &[(1, 9)]);
                if removed && !dropped {
                    assert!(active_picker(&mut cx).is_none());
                    cx.assert_editor_state(SOURCE);
                    cx.update_workspace(|workspace, _, cx| {
                        assert_eq!(workspace.items(cx).count(), 0);
                    });
                    assert_no_picker_notifications(&mut cx);
                } else {
                    assert_no_navigation(&mut cx);
                }
            }
        }
    }

    #[gpui::test]
    async fn test_open_definition_locations_honors_setting_without_query(cx: &mut TestAppContext) {
        cx.update(crate::init);
        for picker_kind in KINDS
            .into_iter()
            .filter(|kind| *kind != LspPickerKind::References)
        {
            let kind = definition_kind(picker_kind);
            for open_results_in in [OpenResultsIn::Picker, OpenResultsIn::MultiBuffer] {
                let mut cx = definition_cx(cx).await;
                let definitions = track_definition_requests(&mut cx, kind, &[]);
                let references = track_reference_requests(&mut cx);
                let locations = source_definition_locations(&mut cx);
                set_results(&mut cx, open_results_in);

                dispatch_definition_locations(&mut cx, kind, locations.clone());
                if open_results_in == OpenResultsIn::Picker {
                    assert_picker_locations(&mut cx, picker_kind, TARGETS);
                    dispatch_definition_locations(&mut cx, kind, locations);
                }

                assert_requests(&definitions, &[]);
                assert_requests(&references, &[]);
                if open_results_in == OpenResultsIn::Picker {
                    assert_picker_locations(&mut cx, picker_kind, TARGETS);
                    cx.assert_editor_state(SOURCE);
                } else {
                    assert_native_definition_locations(&mut cx, TARGETS);
                }
            }
        }
    }

    #[gpui::test]
    async fn test_open_definition_locations_fileless_results_use_native_navigation(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let definitions = track_definition_requests(&mut cx, GotoDefinitionKind::Symbol, &[]);
        let references = track_reference_requests(&mut cx);
        let (buffer, locations) = cx.update(|_, cx| {
            let buffer = cx.new(|cx| Buffer::local("abc", cx));
            let locations = buffer_locations(&buffer, &[(0, 0, 1), (0, 2, 3)], cx);
            (buffer, locations)
        });

        let source = source_definition_locations(&mut cx);
        let fileless = cx.update(|_, cx| {
            buffer_locations(&buffer, &[(0, 0, 3)], cx)
                .pop()
                .expect("fileless target")
        });
        let declined_actions = Arc::new(Mutex::new(Some(0)));
        cx.update_workspace(|workspace, _, cx| {
            let declined_actions = declined_actions.clone();
            workspace.register_action_renderer(move |div, _, _, _| {
                let declined_actions = declined_actions.clone();
                div.capture_action(move |_: &OpenDefinitionLocations, _, cx| {
                    if let Some(count) = declined_actions.lock().expect("declined actions").as_mut()
                    {
                        *count += 1;
                        cx.stop_propagation();
                    }
                })
            });
            cx.notify();
        });
        cx.update(|window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        });
        for (index, declined) in [
            Vec::new(),
            vec![source[0].clone()],
            vec![source[0].clone(), source[0].clone()],
            vec![fileless.clone()],
            vec![source[0].clone(), fileless, source[1].clone()],
        ]
        .into_iter()
        .enumerate()
        {
            let action = cx.update_editor(|editor, _, cx| {
                definition_action(editor, GotoDefinitionKind::Symbol, declined, cx)
            });
            cx.editor(|editor, _, _| assert!(action.0.source.request.is_current(editor)));
            cx.dispatch_action(action);
            assert_eq!(
                *declined_actions.lock().expect("declined actions"),
                Some(index + 1)
            );
            assert_no_navigation(&mut cx);
        }
        *declined_actions.lock().expect("declined actions") = None;
        dispatch_definition_locations(&mut cx, GotoDefinitionKind::Symbol, locations);

        assert!(active_picker(&mut cx).is_none());
        cx.assert_editor_state(SOURCE);
        cx.update_workspace(|workspace, _, cx| {
            assert_eq!(workspace.panes().len(), 1);
            let target = workspace
                .active_item_as::<Editor>(cx)
                .expect("target editor");
            assert_eq!(
                target.read(cx).buffer().read(cx).as_singleton(),
                Some(buffer)
            );
            assert_selection(&target, &[(0, 0, 1), (0, 2, 3)], cx);
        });
        assert_requests(&definitions, &[]);
        assert_requests(&references, &[]);
    }

    #[gpui::test]
    async fn test_open_definition_locations_wrong_or_dropped_source_propagates(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        for (separate_workspace, drop_source) in [(false, false), (false, true), (true, false)] {
            let mut source_cx = definition_cx(cx).await;
            let mut receiver_cx = definition_cx(cx).await;
            let definitions =
                track_definition_requests(&mut receiver_cx, GotoDefinitionKind::Symbol, &[]);
            let references = track_reference_requests(&mut receiver_cx);
            let source_references = track_reference_requests(&mut source_cx);
            let locations = source_definition_locations(&mut receiver_cx);
            let source = if separate_workspace {
                assert_ne!(source_cx.workspace, receiver_cx.workspace);
                source_cx.editor.clone()
            } else {
                receiver_cx.update(|window, cx| {
                    let buffer = cx.new(|cx| Buffer::local("abc", cx));
                    cx.new(|cx| Editor::for_buffer(buffer, None, window, cx))
                })
            };
            if drop_source {
                receiver_cx.update_workspace(|workspace, window, cx| {
                    source.update(cx, |editor, cx| {
                        editor.added_to_workspace(workspace, window, cx)
                    });
                });
            }
            receiver_cx.run_until_parked();
            let action = receiver_cx.update(|_, cx| {
                source.update(cx, |editor, cx| {
                    definition_action(editor, GotoDefinitionKind::Symbol, locations, cx)
                })
            });
            let source = (!drop_source).then_some(source);
            if drop_source {
                assert!(action.0.source.editor.upgrade().is_none());
            }
            assert_eq!(dispatch_with_propagation(&mut receiver_cx, &action), 1);
            assert_requests(&definitions, &[]);
            assert_requests(&references, &[]);
            assert_requests(&source_references, &[]);
            assert_no_navigation(&mut source_cx);
            assert_no_navigation(&mut receiver_cx);
            if drop_source {
                assert!(action.0.source.editor.upgrade().is_none());
            }
            drop(source);
        }
    }

    #[gpui::test]
    async fn test_picker_confirmation_preserves_captured_origin_after_caret_moves(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        for (command_click, reopen) in [(false, false), (true, false), (true, true)] {
            let mut cx = definition_cx(cx).await;
            let requests = track_query(&mut cx, LspPickerKind::Definition, TARGETS, 1);
            if command_click {
                assert_hover_cleared(&mut cx);
                click(&mut cx, SOURCE, Modifiers::secondary_key());
            } else {
                let editor = cx.editor.clone();
                cx.update_workspace(|workspace, window, cx| {
                    LspLocationsPicker::open(
                        LspPickerKind::Definition,
                        editor,
                        workspace,
                        window,
                        cx,
                    );
                });
            }
            let moved_caret = SOURCE.replace("aˇbc", "abc").replace("xyz", "xˇyz");
            cx.set_selections_state(&moved_caret);
            cx.run_until_parked();
            assert_requests(&requests, &[(1, 9)]);
            cx.assert_editor_state(&moved_caret);
            assert!(active_picker(&mut cx).is_none());
            advance(&mut cx);
            assert_picker_locations(&mut cx, LspPickerKind::Definition, TARGETS);
            cx.assert_editor_state(&moved_caret);
            let pending = reopen.then(|| {
                let modal = active_picker(&mut cx).expect("original picker");
                cx.update_workspace(|workspace, window, cx| {
                    workspace.hide_modal(window, cx);
                });
                cx.run_until_parked();
                let pending = track_query(&mut cx, LspPickerKind::Definition, LATEST_TARGETS, 1);
                click(&mut cx, SECOND_SOURCE, Modifiers::secondary_key());
                assert_requests(&pending, &[(2, 15)]);
                cx.dispatch_action(workspace::ReopenLastPicker);
                assert_eq!(active_picker(&mut cx), Some(modal));
                pending
            });
            confirm_picker(&mut cx);
            if let Some(pending) = pending {
                advance(&mut cx);
                assert_requests(&pending, &[(2, 15)]);
            }
            assert_native_definition_locations(&mut cx, &TARGETS[..1]);
            cx.dispatch_action(GoToOlderTag);
            cx.run_until_parked();
            cx.assert_editor_state(if command_click { SOURCE } else { &moved_caret });
            cx.dispatch_action(GoToNewerTag);
            cx.run_until_parked();
            cx.assert_editor_state(&SOURCE.replace("aˇbc", "abc").replace("main", "mainˇ"));
            assert_requests(&requests, &[(1, 9)]);
        }
    }

    #[gpui::test]
    async fn test_cold_cmd_click_query_error_falls_back_to_references(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let (requests, _) = track_query_response(&mut cx, LspPickerKind::Definition, |_| {
            (1, Err(anyhow::anyhow!("definition query failed")))
        });
        let references = track_reference_requests(&mut cx);
        assert_hover_cleared(&mut cx);
        click(&mut cx, SOURCE, Modifiers::secondary_key());
        assert_requests(&requests, &[(1, 9)]);
        assert_requests(&references, &[]);
        assert_no_navigation(&mut cx);
        advance(&mut cx);
        assert_requests(&requests, &[(1, 9)]);
        assert_requests(&references, &[(1, 9)]);
        assert_picker_locations(
            &mut cx,
            LspPickerKind::References,
            &[(1, 8, 11), (2, 14, 17)],
        );
        cx.assert_editor_state(SOURCE);
        assert_hover_cleared(&mut cx);
        assert_no_picker_notifications(&mut cx);
    }

    #[gpui::test]
    async fn test_cold_cmd_click_latest_request_owns_picker(cx: &mut TestAppContext) {
        cx.update(crate::init);
        for (empty, keyboard, separate_editor, latest_first) in [
            (false, false, false, true),
            (true, false, false, true),
            (false, true, false, true),
            (false, false, true, false),
            (false, false, true, true),
            (true, false, true, false),
            (true, false, true, true),
            (false, true, true, false),
            (false, true, true, true),
        ] {
            let mut cx = definition_cx(cx).await;
            let other = separate_editor.then(|| add_other_editor(&mut cx, true));
            let locations = source_definition_locations(&mut cx);
            let stale = cx.update_editor(|editor, _, cx| {
                definition_action(
                    editor,
                    GotoDefinitionKind::Symbol,
                    if empty { Vec::new() } else { locations },
                    cx,
                )
            });
            let uri = cx.buffer_lsp_url.clone();
            let (requests, responses) =
                track_query_response(&mut cx, LspPickerKind::Definition, move |position| {
                    let (delay, ranges) = match (position.line, position.character) {
                        (1, 9) => (
                            if latest_first { 2 } else { 1 },
                            if empty {
                                &[][..]
                            } else {
                                &[(0, 3, 7), (2, 8, 11)][..]
                            },
                        ),
                        (2, 15) => (if latest_first { 1 } else { 2 }, LATEST_TARGETS),
                        _ => panic!("unexpected request: {position:?}"),
                    };
                    (delay, Ok(self::references(uri.clone(), ranges)))
                });
            let references = track_reference_requests(&mut cx);
            assert_hover_cleared(&mut cx);
            click(&mut cx, SOURCE, Modifiers::secondary_key());
            assert_requests(&requests, &[(1, 9)]);
            cx.editor(|editor, _, _| assert!(!stale.0.source.request.is_current(editor)));
            assert_eq!(dispatch_with_propagation(&mut cx, &stale), 0);
            assert!(active_picker(&mut cx).is_none());
            cx.assert_editor_state(SOURCE);
            assert_no_picker_notifications(&mut cx);
            assert_requests(&references, &[]);
            if let Some(other) = other {
                cx.update_workspace(|workspace, window, cx| {
                    assert!(workspace.activate_item(&other, true, true, window, cx));
                });
                cx.editor = other;
                cx.run_until_parked();
            }
            if keyboard {
                cx.set_selections_state(SECOND_SOURCE);
                dispatch_query(&mut cx, LspPickerKind::Definition, None);
            } else {
                click(&mut cx, SECOND_SOURCE, Modifiers::secondary_key());
            }
            assert_requests(&requests, &[(1, 9), (2, 15)]);
            assert_requests(&responses, &[]);
            assert_requests(&references, &[]);
            assert!(active_picker(&mut cx).is_none());
            cx.assert_editor_state(SECOND_SOURCE);
            assert_hover_cleared(&mut cx);
            let mut picker = None;
            let responses_in_order = if !latest_first {
                [&[(1, 9)][..], &[(1, 9), (2, 15)][..]]
            } else if separate_editor {
                [&[(2, 15)][..], &[(2, 15), (1, 9)][..]]
            } else {
                [&[(2, 15)][..], &[(2, 15)][..]]
            };
            for (index, expected) in responses_in_order.into_iter().enumerate() {
                advance(&mut cx);
                assert_requests(&responses, expected);
                assert_requests(&requests, &[(1, 9), (2, 15)]);
                assert_requests(&references, &[]);
                if !latest_first && index == 0 {
                    assert!(active_picker(&mut cx).is_none());
                } else {
                    assert_picker_locations(&mut cx, LspPickerKind::Definition, LATEST_TARGETS);
                    let current = active_picker(&mut cx).expect("latest picker");
                    assert_eq!(&current, picker.get_or_insert(current.clone()));
                }
                cx.assert_editor_state(SECOND_SOURCE);
                assert_hover_cleared(&mut cx);
                assert_no_picker_notifications(&mut cx);
            }
        }
    }

    #[gpui::test]
    async fn test_cold_cmd_click_hidden_source_opens_picker(cx: &mut TestAppContext) {
        cx.update(crate::init);
        for (fallback, removed, keyboard) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, true, true),
        ] {
            let mut cx = definition_cx(cx).await;
            let source = cx.editor.clone();
            let other = add_other_editor(&mut cx, false);
            let requests = track_query(
                &mut cx,
                LspPickerKind::Definition,
                if fallback { &[] } else { TARGETS },
                1,
            );
            let references = track_query(&mut cx, LspPickerKind::References, TARGETS, 0);
            if keyboard {
                dispatch_query(&mut cx, LspPickerKind::Definition, None);
            } else {
                click(&mut cx, SOURCE, Modifiers::secondary_key());
            }
            assert_requests(&requests, &[(1, 9)]);
            assert_no_navigation(&mut cx);
            cx.set_selections_state(SECOND_SOURCE);
            cx.update_workspace(|workspace, window, cx| {
                assert!(workspace.activate_item(&other, true, true, window, cx));
                if removed {
                    workspace.active_pane().update(cx, |pane, cx| {
                        pane.remove_item(source.entity_id(), false, false, window, cx);
                    });
                }
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.refresh();
                let _ = window.draw(cx);
                assert!(other.read(cx).focus_handle(cx).is_focused(window));
                assert!(!source.read(cx).focus_handle(cx).is_focused(window));
            });
            advance(&mut cx);
            assert_requests(&requests, &[(1, 9)]);
            assert_requests(
                &references,
                if fallback && !removed { &[(1, 9)] } else { &[] },
            );
            cx.assert_editor_state(SECOND_SOURCE);
            assert_no_picker_notifications(&mut cx);
            cx.update_workspace(|workspace, _, cx| {
                assert_eq!(workspace.active_item_as::<Editor>(cx), Some(other.clone()));
            });
            if removed {
                assert!(active_picker(&mut cx).is_none());
                continue;
            }
            let kind = if fallback {
                LspPickerKind::References
            } else {
                LspPickerKind::Definition
            };
            assert_picker_locations(&mut cx, kind, TARGETS);
            let picker = active_picker(&mut cx).expect("hidden source picker");
            cx.update(|_, cx| {
                let delegate = &picker.read(cx).picker.read(cx).delegate;
                assert_eq!(delegate.editor, source.downgrade());
                assert!(delegate.origin.is_some());
            });
            confirm_picker(&mut cx);
            assert_native_definition_locations(&mut cx, &TARGETS[..1]);
            cx.dispatch_action(GoToOlderTag);
            cx.assert_editor_state(SOURCE);
        }
    }

    async fn rust_cx(
        capabilities: lsp::ServerCapabilities,
        cx: &mut TestAppContext,
    ) -> EditorLspTestContext {
        EditorLspTestContext::new_rust(capabilities, cx).await
    }

    fn open(cx: &mut EditorLspTestContext, kind: LspPickerKind) {
        let editor = cx.editor.clone();
        let workspace = cx.workspace.clone();
        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                LspLocationsPicker::open(kind, editor, workspace, window, cx);
            });
        });
        cx.run_until_parked();
    }

    fn active_picker(cx: &mut EditorLspTestContext) -> Option<Entity<LspLocationsPicker>> {
        let workspace = cx.workspace.clone();
        cx.update(|_window, cx| workspace.read(cx).active_modal::<LspLocationsPicker>(cx))
    }

    fn set_picker(cx: &mut EditorLspTestContext) {
        cx.update(|_window, cx| {
            cx.update_global::<settings::SettingsStore, _>(|settings, cx| {
                settings.update_user_settings(cx, |settings| {
                    settings.editor.lsp_results_location = Some(OpenResultsIn::Picker);
                });
            });
        });
    }

    fn definitions(uri: lsp::Uri, ranges: &[(u32, u32, u32)]) -> lsp::GotoDefinitionResponse {
        lsp::GotoDefinitionResponse::Array(references(uri, ranges))
    }

    fn references(uri: lsp::Uri, ranges: &[(u32, u32, u32)]) -> Vec<lsp::Location> {
        ranges
            .iter()
            .map(|&(row, start, end)| lsp::Location {
                uri: uri.clone(),
                range: lsp::Range::new(
                    lsp::Position::new(row, start),
                    lsp::Position::new(row, end),
                ),
            })
            .collect()
    }

    async fn definition_cx(cx: &mut TestAppContext) -> EditorLspTestContext {
        let mut cx = rust_cx(
            lsp::ServerCapabilities {
                declaration_provider: Some(lsp::DeclarationCapability::Simple(true)),
                implementation_provider: Some(lsp::ImplementationProviderCapability::Simple(true)),
                definition_provider: Some(lsp::OneOf::Left(true)),
                type_definition_provider: Some(lsp::TypeDefinitionProviderCapability::Simple(true)),
                references_provider: Some(lsp::OneOf::Left(true)),
                ..lsp::ServerCapabilities::default()
            },
            cx,
        )
        .await;
        set_results(&mut cx, OpenResultsIn::Picker);
        cx.set_state(SOURCE);
        cx
    }

    fn definition_modifiers(kind: GotoDefinitionKind) -> Modifiers {
        Modifiers {
            shift: kind == GotoDefinitionKind::Type,
            ..Modifiers::secondary_key()
        }
    }

    fn track_definition_requests(
        cx: &mut EditorLspTestContext,
        kind: GotoDefinitionKind,
        ranges: &[(u32, u32, u32)],
    ) -> Arc<Mutex<Vec<lsp::Position>>> {
        let kind = match kind {
            GotoDefinitionKind::Symbol => LspPickerKind::Definition,
            GotoDefinitionKind::Type => LspPickerKind::TypeDefinition,
            GotoDefinitionKind::Declaration => LspPickerKind::Declaration,
            GotoDefinitionKind::Implementation => LspPickerKind::Implementation,
        };
        track_query(cx, kind, ranges, 0)
    }

    async fn assert_cached_definition_picker(kind: GotoDefinitionKind, cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mut cx = definition_cx(cx).await;
        let requests = track_definition_requests(
            &mut cx,
            kind,
            &[(2, 14, 17), (1, 8, 11), (0, 3, 7), (2, 14, 17)],
        );
        let position = cx.pixel_position(SOURCE);
        let modifiers = definition_modifiers(kind);
        cx.simulate_mouse_move(position, None, modifiers);
        cx.run_until_parked();
        assert_definition_highlight(&mut cx);
        assert_requests(&requests, &[(1, 9)]);
        assert!(active_picker(&mut cx).is_none());

        cx.simulate_click(position, modifiers);
        cx.run_until_parked();

        assert_requests(&requests, &[(1, 9)]);
        assert_picker_locations(
            &mut cx,
            if kind == GotoDefinitionKind::Type {
                LspPickerKind::TypeDefinition
            } else {
                LspPickerKind::Definition
            },
            &[(0, 3, 7), (2, 14, 17)],
        );
        assert_hover_cleared(&mut cx);
    }

    fn assert_definition_highlight(cx: &mut EditorLspTestContext) {
        cx.assert_editor_text_highlights(
            HighlightKey::HoveredLinkState,
            indoc! {r#"
            fn main() {
                let «abc» = 123;
                let xyz = abc;
            }
        "#},
        );
    }

    fn assert_hover_cleared(cx: &mut EditorLspTestContext) {
        cx.editor(|editor, _, cx| {
            assert_eq!(
                editor
                    .text_highlights(HighlightKey::HoveredLinkState, cx)
                    .map(|(_, ranges)| ranges.len()),
                None
            );
        });
    }

    fn assert_picker_locations(
        cx: &mut EditorLspTestContext,
        kind: LspPickerKind,
        ranges: &[(u32, u32, u32)],
    ) {
        let modal = active_picker(cx).expect("locations picker");
        cx.update(|_, cx| {
            let delegate = &modal.read(cx).picker.read(cx).delegate;
            assert_eq!(delegate.kind, kind);
            let actual = delegate
                .all_matches
                .iter()
                .map(|location| {
                    let snapshot = location.buffer.read(cx).snapshot();
                    let start = snapshot.summary_for_anchor::<Point>(&location.anchor_range.start);
                    let end = snapshot.summary_for_anchor::<Point>(&location.anchor_range.end);
                    assert_eq!(snapshot.offset_to_point(location.range.start), start);
                    assert_eq!(snapshot.offset_to_point(location.range.end), end);
                    (location.path.path.as_unix_str().to_string(), start..end)
                })
                .collect::<Vec<_>>();
            let expected = ranges
                .iter()
                .map(|&(row, start, end)| {
                    (
                        "dir/file.rs".to_string(),
                        Point::new(row, start)..Point::new(row, end),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        });
    }

    fn reset_navigation_editor(cx: &mut EditorLspTestContext, source: &str) {
        let previous = cx.editor.clone();
        cx.editor = cx.update_workspace(|workspace, window, cx| {
            workspace.hide_modal(window, cx);
            let buffer = previous.read(cx).buffer().clone();
            let project = workspace.project().clone();
            let editor = cx.new(|cx| Editor::for_multibuffer(buffer, Some(project), window, cx));
            workspace.active_pane().update(cx, |pane, cx| {
                pane.remove_item(previous.entity_id(), false, false, window, cx);
            });
            workspace.add_item_to_active_pane(Box::new(editor.clone()), None, true, window, cx);
            editor
        });
        drop(previous);
        cx.set_selections_state(source);
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(workspace::SERIALIZATION_THROTTLE_TIME);
        cx.run_until_parked();
    }

    fn track_reference_requests(cx: &mut EditorLspTestContext) -> Arc<Mutex<Vec<lsp::Position>>> {
        track_query(cx, LspPickerKind::References, &[(1, 8, 11), (2, 14, 17)], 0)
    }

    fn assert_no_navigation(cx: &mut EditorLspTestContext) {
        assert_no_navigation_at(cx, SOURCE);
    }

    fn assert_no_navigation_at(cx: &mut EditorLspTestContext, source: &str) {
        assert_no_picker_notifications(cx);
        cx.assert_editor_state(source);
        assert_hover_cleared(cx);
        assert!(active_picker(cx).is_none());
        let source_editor = cx.editor.clone();
        cx.update_workspace(|workspace, _, cx| {
            assert_eq!(workspace.panes().len(), 1);
            assert_eq!(workspace.active_item_as::<Editor>(cx), Some(source_editor));
        });
    }

    fn track_query(
        cx: &mut EditorLspTestContext,
        kind: LspPickerKind,
        ranges: &[(u32, u32, u32)],
        delay: u64,
    ) -> Arc<Mutex<Vec<lsp::Position>>> {
        let locations = references(cx.buffer_lsp_url.clone(), ranges);
        track_query_response(cx, kind, move |_| (delay, Ok(locations.clone()))).0
    }

    fn track_query_response(
        cx: &mut EditorLspTestContext,
        kind: LspPickerKind,
        response: impl Fn(lsp::Position) -> (u64, anyhow::Result<Vec<lsp::Location>>) + Send + 'static,
    ) -> (
        Arc<Mutex<Vec<lsp::Position>>>,
        Arc<Mutex<Vec<lsp::Position>>>,
    ) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let responses = Arc::new(Mutex::new(Vec::new()));
        let handler = {
            let requests = requests.clone();
            let responses = responses.clone();
            let uri = cx.buffer_lsp_url.clone();
            move |params: lsp::TextDocumentPositionParams, cx: gpui::AsyncApp| {
                assert_eq!(params.text_document.uri, uri);
                requests.lock().expect("requests").push(params.position);
                let (delay, result) = response(params.position);
                let responses = responses.clone();
                async move {
                    if delay > 0 {
                        cx.background_executor()
                            .timer(Duration::from_secs(delay))
                            .await;
                    }
                    responses.lock().expect("responses").push(params.position);
                    result.map(Some)
                }
            }
        };
        if kind == LspPickerKind::References {
            cx.lsp
                .set_request_handler::<lsp::request::References, _, _>(move |params, cx| {
                    handler(params.text_document_position, cx)
                });
        } else {
            let handler = move |params: lsp::GotoDefinitionParams, cx| {
                let result = handler(params.text_document_position_params, cx);
                async move { Ok(result.await?.map(lsp::GotoDefinitionResponse::Array)) }
            };
            match kind {
                LspPickerKind::Definition => cx
                    .lsp
                    .set_request_handler::<lsp::request::GotoDefinition, _, _>(handler),
                LspPickerKind::Declaration => cx
                    .lsp
                    .set_request_handler::<lsp::request::GotoDeclaration, _, _>(handler),
                LspPickerKind::Implementation => cx
                    .lsp
                    .set_request_handler::<lsp::request::GotoImplementation, _, _>(handler),
                LspPickerKind::TypeDefinition => cx
                    .lsp
                    .set_request_handler::<lsp::request::GotoTypeDefinition, _, _>(handler),
                LspPickerKind::References => unreachable!(),
            };
        }
        (requests, responses)
    }

    fn definition_kind(kind: LspPickerKind) -> GotoDefinitionKind {
        match kind {
            LspPickerKind::Definition => GotoDefinitionKind::Symbol,
            LspPickerKind::Declaration => GotoDefinitionKind::Declaration,
            LspPickerKind::Implementation => GotoDefinitionKind::Implementation,
            LspPickerKind::TypeDefinition => GotoDefinitionKind::Type,
            LspPickerKind::References => unreachable!(),
        }
    }

    fn assert_no_requests(requests: &[Arc<Mutex<Vec<lsp::Position>>>]) {
        for requests in requests {
            assert_requests(requests, &[]);
        }
    }

    fn dispatch_query(
        cx: &mut EditorLspTestContext,
        kind: LspPickerKind,
        location: Option<OpenResultsIn>,
    ) {
        cx.update(|window, cx| window.dispatch_action(query_action(kind, location), cx));
        cx.run_until_parked();
    }

    fn disable_picker_lsp_data(editor: &mut Editor, mode: Option<EditorMode>) {
        assert!(editor.lsp_data_enabled());
        if let Some(mode) = mode {
            editor.set_mode(mode);
        } else {
            editor.disable_lsp_data();
        }
        assert!(!editor.lsp_data_enabled());
    }

    fn assert_no_picker_notifications(cx: &mut EditorLspTestContext) {
        cx.update_workspace(|workspace, _, _| assert_eq!(workspace.notification_ids(), Vec::new()));
    }

    fn definition_action(
        editor: &Editor,
        kind: GotoDefinitionKind,
        locations: Vec<Location>,
        cx: &Context<Editor>,
    ) -> OpenDefinitionLocations {
        OpenDefinitionLocations(Arc::new(DefinitionLocations {
            source: Arc::new(NavigationSource {
                editor: cx.weak_entity(),
                position: editor.selections.newest_anchor().head(),
                origin: None,
                request: editor.navigation_request(),
            }),
            kind,
            locations,
        }))
    }

    fn dispatch_definition_locations(
        cx: &mut EditorLspTestContext,
        kind: GotoDefinitionKind,
        locations: Vec<Location>,
    ) {
        let action =
            cx.update_editor(|editor, _, cx| definition_action(editor, kind, locations, cx));
        cx.dispatch_action(action);
    }

    fn assert_native_definition_locations(
        cx: &mut EditorLspTestContext,
        ranges: &[(u32, u32, u32)],
    ) {
        assert!(active_picker(cx).is_none());
        let source = cx.editor.clone();
        cx.update_workspace(|workspace, _, cx| {
            assert_eq!(workspace.panes().len(), 1);
            assert_eq!(workspace.active_item_as::<Editor>(cx), Some(source.clone()));
            assert_selection(&source, ranges, cx);
        });
    }

    fn assert_selection(editor: &Entity<Editor>, ranges: &[(u32, u32, u32)], cx: &mut App) {
        editor.update(cx, |editor, cx| {
            assert_eq!(
                editor
                    .selections
                    .all::<Point>(&editor.display_snapshot(cx))
                    .into_iter()
                    .map(|selection| selection.range())
                    .collect::<Vec<_>>(),
                ranges
                    .iter()
                    .map(|&(row, start, end)| Point::new(row, start)..Point::new(row, end))
                    .collect::<Vec<_>>()
            );
        });
    }

    fn source_definition_locations(cx: &mut EditorLspTestContext) -> Vec<Location> {
        cx.multibuffer(|buffer, cx| {
            buffer_locations(
                &buffer.as_singleton().expect("singleton buffer"),
                TARGETS,
                cx,
            )
        })
    }

    fn buffer_locations(
        buffer: &Entity<Buffer>,
        ranges: &[(u32, u32, u32)],
        cx: &App,
    ) -> Vec<Location> {
        let snapshot = buffer.read(cx).snapshot();
        ranges
            .iter()
            .map(|&(row, start, end)| Location {
                buffer: buffer.clone(),
                range: snapshot.anchor_before(Point::new(row, start))
                    ..snapshot.anchor_after(Point::new(row, end)),
            })
            .collect()
    }

    fn assert_requests(requests: &Arc<Mutex<Vec<lsp::Position>>>, expected: &[(u32, u32)]) {
        assert_eq!(
            *requests.lock().expect("requests"),
            expected
                .iter()
                .map(|&(row, column)| lsp::Position::new(row, column))
                .collect::<Vec<_>>()
        );
    }

    fn add_other_editor(cx: &mut EditorLspTestContext, same_buffer: bool) -> Entity<Editor> {
        let source = cx.editor.clone();
        let other = cx.update_workspace(|workspace, window, cx| {
            let project = workspace.project().clone();
            let other = if same_buffer {
                let buffer = source.read(cx).buffer().clone();
                cx.new(|cx| Editor::for_multibuffer(buffer, Some(project), window, cx))
            } else {
                let buffer = cx.new(|cx| Buffer::local("other tab", cx));
                cx.new(|cx| Editor::for_buffer(buffer, Some(project), window, cx))
            };
            workspace.active_pane().update(cx, |pane, _| {
                pane.unpreview_item_if_preview(source.entity_id())
            });
            let source_pane = workspace.active_pane().clone();
            let pane = if same_buffer {
                workspace.adjacent_pane_of(&source_pane, window, cx)
            } else {
                source_pane
            };
            workspace.add_item(pane, Box::new(other.clone()), None, true, true, window, cx);
            assert!(workspace.activate_item(&source, true, true, window, cx));
            other
        });
        cx.run_until_parked();
        other
    }

    fn confirm_picker(cx: &mut EditorLspTestContext) {
        cx.update(|window, cx| {
            let confirm = cx
                .build_action("menu::Confirm", None)
                .expect("confirm action");
            window.dispatch_action(confirm, cx);
        });
        cx.run_until_parked();
    }

    fn click(cx: &mut EditorLspTestContext, source: &str, modifiers: Modifiers) {
        let position = cx.pixel_position(source);
        cx.simulate_click(position, modifiers);
        cx.simulate_modifiers_change(Modifiers::none());
        cx.run_until_parked();
    }

    fn advance(cx: &mut EditorLspTestContext) {
        cx.background_executor.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }

    fn set_results(cx: &mut EditorLspTestContext, location: OpenResultsIn) {
        cx.update(|_, cx| {
            cx.update_global::<settings::SettingsStore, _>(|settings, cx| {
                settings.update_user_settings(cx, |settings| {
                    settings.editor.lsp_results_location = Some(location);
                    settings.editor.go_to_definition_fallback =
                        Some(GoToDefinitionFallback::FindAllReferences);
                });
            })
        });
    }

    fn query_action(
        kind: LspPickerKind,
        open_results_in: Option<OpenResultsIn>,
    ) -> Box<dyn gpui::Action> {
        match kind {
            LspPickerKind::Definition => Box::new(GoToDefinition { open_results_in }),
            LspPickerKind::Declaration => Box::new(GoToDeclaration { open_results_in }),
            LspPickerKind::Implementation => Box::new(GoToImplementation { open_results_in }),
            LspPickerKind::TypeDefinition => Box::new(GoToTypeDefinition { open_results_in }),
            LspPickerKind::References => Box::new(FindAllReferences {
                open_results_in,
                ..FindAllReferences::default()
            }),
        }
    }

    fn dispatch_with_propagation(
        cx: &mut EditorLspTestContext,
        action: &OpenDefinitionLocations,
    ) -> usize {
        let propagated = Arc::new(AtomicUsize::new(0));
        let workspace = cx.workspace.clone();
        cx.update(|window, cx| {
            App::on_action::<OpenDefinitionLocations>(cx, {
                let propagated = propagated.clone();
                move |_, cx| {
                    propagated.fetch_add(1, Ordering::SeqCst);
                    cx.propagate();
                }
            });
            workspace
                .read(cx)
                .focus_handle(cx)
                .dispatch_action(action, window, cx);
        });
        cx.run_until_parked();
        propagated.load(Ordering::SeqCst)
    }
}
