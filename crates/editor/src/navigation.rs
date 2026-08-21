use super::*;

#[derive(Clone, Default)]
pub struct NavigationRequest(Arc<()>);

impl NavigationRequest {
    pub fn is_current(&self, editor: &Editor) -> bool {
        self == &editor.navigation.request
    }
}

impl PartialEq for NavigationRequest {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub(super) struct NavigationState {
    request: NavigationRequest,
    task: Task<()>,
    reference_sources: Vec<(Anchor, NavigationRequest)>,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            request: NavigationRequest::default(),
            task: Task::ready(()),
            reference_sources: Vec::new(),
        }
    }
}

impl Editor {
    pub fn begin_navigation(&mut self) -> NavigationRequest {
        self.navigation.task = Task::ready(());
        self.navigation.request = NavigationRequest::default();
        self.navigation.reference_sources.clear();
        self.navigation_request()
    }

    pub fn navigation_request(&self) -> NavigationRequest {
        self.navigation.request.clone()
    }

    pub fn run_navigation_task<T: 'static>(
        &mut self,
        task: Task<anyhow::Result<T>>,
        cx: &mut Context<Self>,
    ) {
        self.navigation.task = cx.spawn(async move |_, _| {
            task.await.log_err();
        });
    }

    pub fn move_left(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                let cursor = if selection.is_empty() {
                    movement::left(map, selection.start)
                } else {
                    selection.start
                };
                selection.collapse_to(cursor, SelectionGoal::None);
            });
        })
    }

    pub fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| (movement::left(map, head), SelectionGoal::None));
        })
    }

    pub fn move_right(&mut self, _: &MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                let cursor = if selection.is_empty() {
                    movement::right(map, selection.end)
                } else {
                    selection.end
                };
                selection.collapse_to(cursor, SelectionGoal::None)
            });
        })
    }

    pub fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (movement::right(map, head), SelectionGoal::None)
            });
        });
    }

    pub fn move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.take_rename(true, window, cx).is_some() {
            return;
        }

        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        let text_layout_details = &self.text_layout_details(window, cx);
        let selection_count = self.selections.count();
        let first_selection = self.selections.first_anchor();

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up(
                    map,
                    selection.start,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });

        if selection_count == 1 && first_selection.range() == self.selections.first_anchor().range()
        {
            cx.propagate();
        }
    }

    pub fn move_up_by_lines(
        &mut self,
        action: &MoveUpByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.take_rename(true, window, cx).is_some() {
            return;
        }

        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up_by_rows(
                    map,
                    selection.start,
                    action.lines,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        })
    }

    pub fn move_down_by_lines(
        &mut self,
        action: &MoveDownByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.take_rename(true, window, cx).is_some() {
            return;
        }

        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down_by_rows(
                    map,
                    selection.start,
                    action.lines,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        })
    }

    pub fn select_down_by_lines(
        &mut self,
        action: &SelectDownByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::down_by_rows(map, head, action.lines, goal, false, text_layout_details)
            })
        })
    }

    pub fn select_up_by_lines(
        &mut self,
        action: &SelectUpByLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::up_by_rows(map, head, action.lines, goal, false, text_layout_details)
            })
        })
    }

    pub fn select_page_up(
        &mut self,
        _: &SelectPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row_count) = self.visible_row_count() else {
            return;
        };

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::up_by_rows(map, head, row_count, goal, false, text_layout_details)
            })
        })
    }

    pub fn move_page_up(
        &mut self,
        action: &MovePageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.take_rename(true, window, cx).is_some() {
            return;
        }

        if self
            .context_menu
            .borrow_mut()
            .as_mut()
            .map(|menu| menu.select_first(self.completion_provider.as_deref(), window, cx))
            .unwrap_or(false)
        {
            return;
        }

        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }

        let Some(row_count) = self.visible_row_count() else {
            return;
        };

        let effects = if action.center_cursor {
            SelectionEffects::scroll(Autoscroll::center())
        } else {
            SelectionEffects::default()
        };

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(effects, window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::up_by_rows(
                    map,
                    selection.end,
                    row_count,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::up(map, head, goal, false, text_layout_details)
            })
        })
    }

    pub fn move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.take_rename(true, window, cx).is_some() {
            return;
        }

        if self.mode.is_single_line() {
            cx.propagate();
            return;
        }

        let text_layout_details = &self.text_layout_details(window, cx);
        let selection_count = self.selections.count();
        let first_selection = self.selections.first_anchor();

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down(
                    map,
                    selection.end,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });

        if selection_count == 1 && first_selection.range() == self.selections.first_anchor().range()
        {
            cx.propagate();
        }
    }

    pub fn select_page_down(
        &mut self,
        _: &SelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row_count) = self.visible_row_count() else {
            return;
        };

        let text_layout_details = &self.text_layout_details(window, cx);

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::down_by_rows(map, head, row_count, goal, false, text_layout_details)
            })
        })
    }

    pub fn move_page_down(
        &mut self,
        action: &MovePageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.take_rename(true, window, cx).is_some() {
            return;
        }

        if self
            .context_menu
            .borrow_mut()
            .as_mut()
            .map(|menu| menu.select_last(self.completion_provider.as_deref(), window, cx))
            .unwrap_or(false)
        {
            return;
        }

        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }

        let Some(row_count) = self.visible_row_count() else {
            return;
        };

        let effects = if action.center_cursor {
            SelectionEffects::scroll(Autoscroll::center())
        } else {
            SelectionEffects::default()
        };

        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(effects, window, cx, |s| {
            s.move_with(&mut |map, selection| {
                if !selection.is_empty() {
                    selection.goal = SelectionGoal::None;
                }
                let (cursor, goal) = movement::down_by_rows(
                    map,
                    selection.end,
                    row_count,
                    selection.goal,
                    false,
                    text_layout_details,
                );
                selection.collapse_to(cursor, goal);
            });
        });
    }

    pub fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        let text_layout_details = &self.text_layout_details(window, cx);
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, goal| {
                movement::down(map, head, goal, false, text_layout_details)
            })
        });
    }

    pub fn move_to_previous_word_start(
        &mut self,
        _: &MoveToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::previous_word_start(map, head, false),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_previous_subword_start(
        &mut self,
        _: &MoveToPreviousSubwordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::previous_subword_start(map, head),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_previous_word_start(
        &mut self,
        _: &SelectToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::previous_word_start(map, head, false),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_previous_subword_start(
        &mut self,
        _: &SelectToPreviousSubwordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::previous_subword_start(map, head),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_next_word_end(
        &mut self,
        _: &MoveToNextWordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::next_word_end(map, head, false),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_next_subword_end(
        &mut self,
        _: &MoveToNextSubwordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (movement::next_subword_end(map, head), SelectionGoal::None)
            });
        })
    }

    pub fn select_to_next_word_end(
        &mut self,
        _: &SelectToNextWordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::next_word_end(map, head, false),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_next_subword_end(
        &mut self,
        _: &SelectToNextSubwordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (movement::next_subword_end(map, head), SelectionGoal::None)
            });
        })
    }

    pub fn move_to_beginning_of_line(
        &mut self,
        action: &MoveToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stop_at_indent = action.stop_at_indent && !self.mode.is_single_line();
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::indented_line_beginning(
                        map,
                        head,
                        action.stop_at_soft_wraps,
                        stop_at_indent,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_beginning_of_line(
        &mut self,
        action: &SelectToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stop_at_indent = action.stop_at_indent && !self.mode.is_single_line();
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::indented_line_beginning(
                        map,
                        head,
                        action.stop_at_soft_wraps,
                        stop_at_indent,
                    ),
                    SelectionGoal::None,
                )
            });
        });
    }

    pub fn move_to_end_of_line(
        &mut self,
        action: &MoveToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_cursors_with(&mut |map, head, _| {
                (
                    movement::line_end(map, head, action.stop_at_soft_wraps),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_end_of_line(
        &mut self,
        action: &SelectToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::line_end(map, head, action.stop_at_soft_wraps),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_start_of_paragraph(
        &mut self,
        _: &MoveToStartOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::start_of_paragraph(map, selection.head(), 1),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_end_of_paragraph(
        &mut self,
        _: &MoveToEndOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::end_of_paragraph(map, selection.head(), 1),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_next_comment_paragraph(
        &mut self,
        _: &MoveToNextCommentParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        // Keep the destination paragraph near the top of the viewport so the
        // whole paragraph below the caret stays visible after a jump.
        self.change_selections(
            SelectionEffects::scroll(Autoscroll::top_relative(5.0)),
            window,
            cx,
            |s| {
                s.move_with(&mut |map, selection| {
                    selection.collapse_to(
                        movement::comment_paragraph(
                            map,
                            selection.head(),
                            workspace::searchable::Direction::Next,
                        ),
                        SelectionGoal::None,
                    )
                });
            },
        )
    }

    pub fn move_to_previous_comment_paragraph(
        &mut self,
        _: &MoveToPreviousCommentParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        // Keep the destination paragraph near the top of the viewport so the
        // whole paragraph below the caret stays visible after a jump.
        self.change_selections(
            SelectionEffects::scroll(Autoscroll::top_relative(5.0)),
            window,
            cx,
            |s| {
                s.move_with(&mut |map, selection| {
                    selection.collapse_to(
                        movement::comment_paragraph(
                            map,
                            selection.head(),
                            workspace::searchable::Direction::Prev,
                        ),
                        SelectionGoal::None,
                    )
                });
            },
        )
    }

    pub fn select_to_start_of_paragraph(
        &mut self,
        _: &SelectToStartOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::start_of_paragraph(map, head, 1),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_end_of_paragraph(
        &mut self,
        _: &SelectToEndOfParagraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::end_of_paragraph(map, head, 1),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_start_of_excerpt(
        &mut self,
        _: &MoveToStartOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::start_of_excerpt(
                        map,
                        selection.head(),
                        workspace::searchable::Direction::Prev,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_start_of_next_excerpt(
        &mut self,
        _: &MoveToStartOfNextExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }

        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::start_of_excerpt(
                        map,
                        selection.head(),
                        workspace::searchable::Direction::Next,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_end_of_excerpt(
        &mut self,
        _: &MoveToEndOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::end_of_excerpt(
                        map,
                        selection.head(),
                        workspace::searchable::Direction::Next,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_end_of_previous_excerpt(
        &mut self,
        _: &MoveToEndOfPreviousExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_with(&mut |map, selection| {
                selection.collapse_to(
                    movement::end_of_excerpt(
                        map,
                        selection.head(),
                        workspace::searchable::Direction::Prev,
                    ),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_start_of_excerpt(
        &mut self,
        _: &SelectToStartOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::start_of_excerpt(map, head, workspace::searchable::Direction::Prev),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_start_of_next_excerpt(
        &mut self,
        _: &SelectToStartOfNextExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::start_of_excerpt(map, head, workspace::searchable::Direction::Next),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_end_of_excerpt(
        &mut self,
        _: &SelectToEndOfExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::end_of_excerpt(map, head, workspace::searchable::Direction::Next),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn select_to_end_of_previous_excerpt(
        &mut self,
        _: &SelectToEndOfPreviousExcerpt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.move_heads_with(&mut |map, head, _| {
                (
                    movement::end_of_excerpt(map, head, workspace::searchable::Direction::Prev),
                    SelectionGoal::None,
                )
            });
        })
    }

    pub fn move_to_beginning(
        &mut self,
        _: &MoveToBeginning,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.change_selections(Default::default(), window, cx, |s| {
            s.select_ranges(vec![Anchor::Min..Anchor::Min]);
        });
    }

    pub fn select_to_beginning(
        &mut self,
        _: &SelectToBeginning,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut selection = self.selections.last::<Point>(&self.display_snapshot(cx));
        selection.set_head(Point::zero(), SelectionGoal::None);
        self.change_selections(Default::default(), window, cx, |s| {
            s.select(vec![selection]);
        });
    }

    pub fn move_to_end(&mut self, _: &MoveToEnd, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        let cursor = self.buffer.read(cx).read(cx).len();
        self.change_selections(Default::default(), window, cx, |s| {
            s.select_ranges(vec![cursor..cursor])
        });
    }

    pub fn set_nav_history(&mut self, nav_history: Option<ItemNavHistory>) {
        self.nav_history = nav_history;
    }

    pub fn save_location(
        &mut self,
        _: &SaveLocation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_nav_history_entry(cx);
    }

    pub fn create_nav_history_entry(&mut self, cx: &mut Context<Self>) {
        self.push_to_nav_history(
            self.selections.newest_anchor().head(),
            None,
            false,
            true,
            cx,
        );
    }

    pub fn expand_excerpts(
        &mut self,
        action: &ExpandExcerpts,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::UpAndDown, cx)
    }

    pub fn expand_excerpts_down(
        &mut self,
        action: &ExpandExcerptsDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::Down, cx)
    }

    pub fn expand_excerpts_up(
        &mut self,
        action: &ExpandExcerptsUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expand_excerpts_for_direction(action.lines, ExpandExcerptDirection::Up, cx)
    }

    pub fn go_to_singleton_buffer_point(
        &mut self,
        point: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_singleton_buffer_range(point..point, window, cx);
    }

    pub fn go_to_singleton_buffer_range(
        &mut self,
        range: Range<Point>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_singleton_buffer_range_impl(range, true, window, cx);
    }

    /// Like `go_to_singleton_buffer_point`, but does not push a navigation
    /// history entry. Useful when the caller already recorded one (e.g. when
    /// a file was just opened and we only need to move the cursor).
    pub fn go_to_singleton_buffer_point_silently(
        &mut self,
        point: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_singleton_buffer_range_impl(point..point, false, window, cx);
    }

    pub fn go_to_next_document_highlight(
        &mut self,
        _: &GoToNextDocumentHighlight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_document_highlight_before_or_after_position(Direction::Next, window, cx);
    }

    pub fn go_to_prev_document_highlight(
        &mut self,
        _: &GoToPreviousDocumentHighlight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_document_highlight_before_or_after_position(Direction::Prev, window, cx);
    }

    pub fn go_to_definition(
        &mut self,
        _: &GoToDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        let position = self.selections.newest_anchor().head();
        let origin = self.navigation_entry(position, cx);
        let definition =
            self.go_to_definition_of_kind(GotoDefinitionKind::Symbol, false, window, cx);
        let request = self.navigation_request();
        let fallback_strategy = EditorSettings::get_global(cx).go_to_definition_fallback;
        cx.spawn_in(window, async move |editor, cx| {
            if definition.await? == Navigated::Yes {
                return Ok(Navigated::Yes);
            }
            if !editor
                .read_with(cx, |editor, _| {
                    editor.lsp_data_enabled() && request.is_current(editor)
                })
                .unwrap_or(false)
            {
                return Ok(Navigated::No);
            }
            match fallback_strategy {
                GoToDefinitionFallback::None => Ok(Navigated::No),
                GoToDefinitionFallback::FindAllReferences => {
                    let query = hover_links::NavigationSource {
                        editor: editor.clone(),
                        position,
                        origin,
                        request,
                    };
                    match editor.update_in(cx, |editor, window, cx| {
                        editor.find_all_references_at(&query, window, cx)
                    }) {
                        Ok(Some(references)) => references.await,
                        Ok(None) | Err(_) => Ok(Navigated::No),
                    }
                }
            }
        })
    }

    pub fn go_to_declaration(
        &mut self,
        _: &GoToDeclaration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        self.go_to_definition_of_kind(GotoDefinitionKind::Declaration, false, window, cx)
    }

    pub fn go_to_declaration_split(
        &mut self,
        _: &GoToDeclarationSplit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        self.go_to_definition_of_kind(GotoDefinitionKind::Declaration, true, window, cx)
    }

    pub fn go_to_implementation(
        &mut self,
        _: &GoToImplementation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        self.go_to_definition_of_kind(GotoDefinitionKind::Implementation, false, window, cx)
    }

    pub fn go_to_implementation_split(
        &mut self,
        _: &GoToImplementationSplit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        self.go_to_definition_of_kind(GotoDefinitionKind::Implementation, true, window, cx)
    }

    pub fn go_to_type_definition(
        &mut self,
        _: &GoToTypeDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        self.go_to_definition_of_kind(GotoDefinitionKind::Type, false, window, cx)
    }

    pub fn go_to_definition_split(
        &mut self,
        _: &GoToDefinitionSplit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        self.go_to_definition_of_kind(GotoDefinitionKind::Symbol, true, window, cx)
    }

    pub fn go_to_type_definition_split(
        &mut self,
        _: &GoToTypeDefinitionSplit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        self.go_to_definition_of_kind(GotoDefinitionKind::Type, true, window, cx)
    }

    pub fn open_url(&mut self, _: &OpenUrl, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selections.newest_anchor();
        let head = selection.head();
        let tail = selection.tail();

        let Some((buffer, start_position)) =
            self.buffer.read(cx).text_anchor_for_position(head, cx)
        else {
            return;
        };

        let end_position = if head != tail {
            let Some((_, pos)) = self.buffer.read(cx).text_anchor_for_position(tail, cx) else {
                return;
            };
            Some(pos)
        } else {
            None
        };

        let url_finder = cx.spawn_in(window, async move |_editor, cx| {
            let url = if let Some(end_pos) = end_position {
                find_url_from_range(&buffer, start_position..end_pos, cx)
            } else {
                find_url(&buffer, start_position, cx).map(|(_, url)| url)
            };

            if let Some(url) = url {
                cx.update(|window, cx| {
                    if parse_zed_link(&url, cx).is_some() {
                        window.dispatch_action(
                            Box::new(zed_actions::OpenZedUrl { url: url.into() }),
                            cx,
                        );
                    } else {
                        cx.open_url(&url);
                    }
                })?;
            }

            anyhow::Ok(())
        });

        url_finder.detach();
    }

    pub fn open_selected_filename(
        &mut self,
        _: &OpenSelectedFilename,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace() else {
            return;
        };

        let position = self.selections.newest_anchor().head();

        let Some((buffer, buffer_position)) =
            self.buffer.read(cx).text_anchor_for_position(position, cx)
        else {
            return;
        };

        let project = self.project.clone();

        cx.spawn_in(window, async move |_, cx| {
            let result = find_file(&buffer, project, buffer_position, cx).await;

            if let Some((_, file_target)) = result {
                let item = workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.open_resolved_path(file_target.resolved_path.clone(), window, cx)
                    })?
                    .await?;

                file_target.navigate_item_to_position(item, cx);
            }
            anyhow::Ok(())
        })
        .detach();
    }

    pub fn go_to_reference_before_or_after_position(
        &mut self,
        direction: Direction,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<()>>> {
        if !self.lsp_data_enabled() {
            return None;
        }
        let request = self.begin_navigation();
        let selection = self.selections.newest_anchor();
        let head = selection.head();

        let multi_buffer = self.buffer.read(cx);

        let (buffer, text_head) = multi_buffer.text_anchor_for_position(head, cx)?;
        let workspace = self.workspace()?;
        let project = workspace.read(cx).project().clone();
        let references =
            project.update(cx, |project, cx| project.references(&buffer, text_head, cx));
        Some(cx.spawn_in(window, async move |editor, cx| -> Result<()> {
            let locations = references.await;
            let Ok(true) = editor.read_with(cx, |editor, _| {
                editor.lsp_data_enabled() && request.is_current(editor)
            }) else {
                return Ok(());
            };
            let Some(locations) = locations? else {
                return Ok(());
            };

            if locations.is_empty() {
                // totally normal - the cursor may be on something which is not
                // a symbol (e.g. a keyword)
                log::info!("no references found under cursor");
                return Ok(());
            }

            let Ok(multi_buffer) = editor.read_with(cx, |editor, _| editor.buffer().clone()) else {
                return Ok(());
            };

            let (locations, current_location_index) =
                multi_buffer.update(cx, |multi_buffer, cx| {
                    let multi_buffer_snapshot = multi_buffer.snapshot(cx);
                    let mut locations = locations
                        .into_iter()
                        .filter_map(|loc| {
                            let start = multi_buffer_snapshot.anchor_in_excerpt(loc.range.start)?;
                            let end = multi_buffer_snapshot.anchor_in_excerpt(loc.range.end)?;
                            Some(start..end)
                        })
                        .collect::<Vec<_>>();
                    // There is an O(n) implementation, but given this list will be
                    // small (usually <100 items), the extra O(log(n)) factor isn't
                    // worth the (surprisingly large amount of) extra complexity.
                    locations
                        .sort_unstable_by(|l, r| l.start.cmp(&r.start, &multi_buffer_snapshot));

                    let head_offset = head.to_offset(&multi_buffer_snapshot);

                    let current_location_index = locations.iter().position(|loc| {
                        loc.start.to_offset(&multi_buffer_snapshot) <= head_offset
                            && loc.end.to_offset(&multi_buffer_snapshot) >= head_offset
                    });

                    (locations, current_location_index)
                });

            let Some(current_location_index) = current_location_index else {
                // This indicates something has gone wrong, because we already
                // handle the "no references" case above
                log::error!(
                    "failed to find current reference under cursor. Total references: {}",
                    locations.len()
                );
                return Ok(());
            };

            let destination_location_index = match direction {
                Direction::Next => (current_location_index + count) % locations.len(),
                Direction::Prev => {
                    (current_location_index + locations.len() - count % locations.len())
                        % locations.len()
                }
            };

            // TODO(cameron): is this needed?
            // the thinking is to avoid "jumping to the current location" (avoid
            // polluting "jumplist" in vim terms)
            if current_location_index == destination_location_index {
                return Ok(());
            }

            let Range { start, end } = locations[destination_location_index];

            editor
                .update_in(cx, |editor, window, cx| {
                    if !editor.lsp_data_enabled() || !request.is_current(editor) {
                        return;
                    }
                    let effects = SelectionEffects::scroll(Autoscroll::for_go_to_definition(
                        editor.cursor_top_offset(cx),
                        cx,
                    ));

                    editor.unfold_ranges(&[start..end], false, false, cx);
                    editor.change_selections(effects, window, cx, |s| {
                        s.select_ranges([start..start]);
                    });
                })
                .ok();

            Ok(())
        }))
    }

    /// Runs the LSP go-to query for `kind` (definition / declaration / type /
    /// implementation) against the symbol under the cursor and returns the raw
    /// target [`Location`]s. The go-to counterpart to
    /// [`Self::find_all_references_locations`]; used by the LSP location pickers.
    pub fn definition_locations_of_kind(
        &mut self,
        kind: GotoDefinitionKind,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Vec<Location>>>> {
        self.definition_locations_of_kind_at(kind, self.selections.newest_anchor().head(), cx)
    }

    pub fn definition_locations_of_kind_at(
        &mut self,
        kind: GotoDefinitionKind,
        position: Anchor,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Vec<Location>>>> {
        if !self.lsp_data_enabled() {
            return None;
        }
        let request = self.navigation_request();
        let provider = self.semantics_provider.clone()?;
        let (buffer, head) = self
            .buffer
            .read(cx)
            .text_anchor_for_position(position, cx)?;
        let definitions = provider.definitions(&buffer, head, kind, cx)?;
        Some(cx.spawn(async move |editor, cx| {
            let definitions = definitions.await;
            let Ok(true) = editor.read_with(cx, |editor, _| {
                editor.lsp_data_enabled() && request.is_current(editor)
            }) else {
                return Ok(Vec::new());
            };
            let definitions = definitions?.unwrap_or_default();
            // Drop a result that points back at the cursor, matching
            // `go_to_definition_of_kind` (otherwise the picker lists the symbol
            // you invoked it on).
            Ok(editor
                .update(cx, |_, cx| {
                    definitions
                        .into_iter()
                        .filter(|link| {
                            hover_links::exclude_link_to_position(&buffer, &head, link, cx)
                        })
                        .map(|link| link.target)
                        .collect()
                })
                .unwrap_or_default())
        }))
    }

    /// Runs an LSP "find all references" query for the symbol under the cursor
    /// and returns the raw [`Location`]s. Unlike [`Self::find_all_references`],
    /// this does not group the results or open any UI
    pub fn find_all_references_locations(
        &mut self,
        project: &Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Vec<Location>>>> {
        self.find_all_references_locations_at(project, self.selections.newest_anchor().head(), cx)
    }

    pub fn find_all_references_locations_at(
        &mut self,
        project: &Entity<Project>,
        position: Anchor,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Vec<Location>>>> {
        if !self.lsp_data_enabled() {
            return None;
        }
        let request = self.navigation_request();
        let (buffer, head) = self
            .buffer
            .read(cx)
            .text_anchor_for_position(position, cx)?;
        let references = project.update(cx, |project, cx| project.references(&buffer, head, cx));
        // Keep every reference, including the one under the cursor, to match the
        // default `find_all_references` multibuffer (`always_open_multibuffer`).
        Some(cx.spawn(async move |editor, cx| {
            let references = references.await;
            let Ok(true) = editor.read_with(cx, |editor, _| {
                editor.lsp_data_enabled() && request.is_current(editor)
            }) else {
                return Ok(Vec::new());
            };
            Ok(references?.unwrap_or_default())
        }))
    }

    pub fn find_all_references(
        &mut self,
        action: &FindAllReferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Navigated>>> {
        self.begin_navigation();
        self.find_all_references_impl(action, None, window, cx)
    }

    pub(crate) fn find_all_references_at(
        &mut self,
        query: &hover_links::NavigationSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Navigated>>> {
        if !query.request.is_current(self) {
            return None;
        }
        self.find_all_references_impl(&FindAllReferences::default(), Some(query), window, cx)
    }

    fn find_all_references_impl(
        &mut self,
        action: &FindAllReferences,
        query: Option<&hover_links::NavigationSource>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<Navigated>>> {
        if !self.lsp_data_enabled() {
            return None;
        }
        let request = self.navigation_request();
        let always_open_multibuffer = action.always_open_multibuffer;
        let selection = self
            .selections
            .newest_anchor()
            .map(|anchor| query.map_or(anchor, |query| query.position));
        let origin = query.and_then(|query| query.origin.clone());
        let multi_buffer = self.buffer.read(cx);
        let multi_buffer_snapshot = multi_buffer.snapshot(cx);
        let selection_offset = selection.map(|anchor| anchor.to_offset(&multi_buffer_snapshot));
        let selection_point = selection.map(|anchor| anchor.to_point(&multi_buffer_snapshot));
        let head = selection_offset.head();

        let head_anchor = multi_buffer_snapshot.anchor_at(
            head,
            if head < selection_offset.tail() {
                Bias::Right
            } else {
                Bias::Left
            },
        );

        let (buffer, head) = multi_buffer.text_anchor_for_position(head, cx)?;
        let workspace = self.workspace()?.downgrade();
        let project = self.project.clone()?;

        match self
            .navigation
            .reference_sources
            .binary_search_by(|(anchor, _)| anchor.cmp(&head_anchor, &multi_buffer_snapshot))
        {
            Ok(index) if self.navigation.reference_sources[index].1.is_current(self) => {
                log::info!(
                    "Ignoring repeated FindAllReferences invocation with the position of already running task"
                );
                return None;
            }
            Ok(index) => {
                self.navigation.reference_sources[index].1 = request.clone();
            }
            Err(index) => {
                self.navigation
                    .reference_sources
                    .insert(index, (head_anchor, request.clone()));
            }
        }

        let references = project.update(cx, |project, cx| project.references(&buffer, head, cx));
        Some(cx.spawn_in(window, async move |editor, cx| {
            let _cleanup = cx.on_drop(&editor, {
                let request = request.clone();
                move |editor, _| {
                    editor
                        .navigation
                        .reference_sources
                        .retain(|(anchor, reservation)| {
                            *anchor != head_anchor || *reservation != request
                        });
                }
            });

            let locations = references.await;
            let Ok(true) = editor.read_with(cx, |editor, _| {
                editor.lsp_data_enabled() && request.is_current(editor)
            }) else {
                return Ok(Navigated::No);
            };
            let Some(locations) = locations? else {
                return anyhow::Ok(Navigated::No);
            };
            let Ok(mut locations) = cx.update(|_, cx| {
                locations
                    .into_iter()
                    .map(|location| {
                        let buffer = location.buffer.read(cx);
                        (location.buffer, location.range.to_point(buffer))
                    })
                    // if special-casing the single-match case, remove ranges
                    // that intersect current selection
                    .filter(|(location_buffer, location)| {
                        if always_open_multibuffer || &buffer != location_buffer {
                            return true;
                        }

                        !location.contains_inclusive(&selection_point.range())
                    })
                    .into_group_map()
            }) else {
                return Ok(Navigated::No);
            };
            if locations.is_empty() {
                return anyhow::Ok(Navigated::No);
            }
            let mut num_locations = 0;
            for ranges in locations.values_mut() {
                ranges.sort_unstable_by_key(|range| (range.start, Reverse(range.end)));
                ranges.dedup();
                num_locations += ranges.len();
            }

            if num_locations == 1 && !always_open_multibuffer {
                let Some((target_buffer, target_ranges)) = locations.into_iter().next() else {
                    return Ok(Navigated::No);
                };
                let Some(target_range) = target_ranges.first() else {
                    return Ok(Navigated::No);
                };
                let position = target_buffer
                    .read_with(cx, |buffer, _| buffer.anchor_before(target_range.start));
                let target = Location {
                    buffer: target_buffer,
                    range: position..position,
                };
                let Ok(navigation) = editor.update_in(cx, |editor, window, cx| {
                    editor.navigate_to_hover_links(
                        None,
                        vec![HoverLink::Text(LocationLink {
                            origin: None,
                            target,
                        })],
                        origin,
                        false,
                        window,
                        cx,
                    )
                }) else {
                    return Ok(Navigated::No);
                };
                return navigation.await;
            }

            Ok(workspace
                .update_in(cx, |workspace, window, cx| {
                    if editor.upgrade().is_none_or(|editor| {
                        let editor = editor.read(cx);
                        !editor.lsp_data_enabled() || !request.is_current(editor)
                    }) {
                        return Navigated::No;
                    }
                    let target = locations
                        .iter()
                        .flat_map(|(k, v)| iter::repeat(k.clone()).zip(v))
                        .map(|(buffer, location)| {
                            buffer
                                .read(cx)
                                .text_for_range(location.clone())
                                .collect::<String>()
                        })
                        .filter(|text| !text.contains('\n'))
                        .unique()
                        .take(3)
                        .join(", ");
                    let title = if target.is_empty() {
                        "References".to_owned()
                    } else {
                        format!("References to {target}")
                    };
                    let allow_preview = PreviewTabsSettings::get_global(cx)
                        .enable_preview_multibuffer_from_code_navigation;
                    let Some(source_item) =
                        Self::containing_item(workspace, editor.entity_id(), cx)
                    else {
                        return Navigated::No;
                    };
                    let Some(source_pane) = workspace.pane_for(source_item.as_ref()) else {
                        return Navigated::No;
                    };
                    let Some((target_editor, target_pane)) =
                        Self::open_locations_in_multibuffer_from_pane(
                            workspace,
                            source_pane,
                            locations,
                            title,
                            false,
                            allow_preview,
                            MultibufferSelectionMode::First,
                            window,
                            cx,
                        )
                    else {
                        return Navigated::No;
                    };
                    let mut history =
                        target_pane.update(cx, |pane, _| pane.nav_history_for_item(&target_editor));
                    target_editor.update(cx, |editor, cx| {
                        let data =
                            editor.navigation_data(editor.selections.newest_anchor().head(), cx);
                        let target = history.navigation_entry(Some(Arc::new(data)));
                        history.push_tag(origin, Some(target));
                    });
                    Navigated::Yes
                })
                .unwrap_or(Navigated::No))
        }))
    }

    pub(super) fn navigation_entry(
        &self,
        cursor_anchor: Anchor,
        cx: &mut Context<Self>,
    ) -> Option<NavigationEntry> {
        let Some(history) = self.nav_history.clone() else {
            return None;
        };
        let data = self.navigation_data(cursor_anchor, cx);
        Some(history.navigation_entry(Some(Arc::new(data) as Arc<dyn Any + Send + Sync>)))
    }

    pub(super) fn push_to_nav_history(
        &mut self,
        cursor_anchor: Anchor,
        new_position: Option<Point>,
        is_deactivate: bool,
        always: bool,
        cx: &mut Context<Self>,
    ) {
        let data = self.navigation_data(cursor_anchor, cx);
        if let Some(nav_history) = self.nav_history.as_mut() {
            if let Some(new_position) = new_position {
                let row_delta = (new_position.row as i64 - data.cursor_position.row as i64).abs();
                if row_delta == 0 || (row_delta < MIN_NAVIGATION_HISTORY_ROW_DELTA && !always) {
                    return;
                }
            }

            let cursor_row = data.cursor_position.row;
            nav_history.push(Some(data), Some(cursor_row), cx);
            cx.emit(EditorEvent::PushedToNavHistory {
                anchor: cursor_anchor,
                is_deactivate,
            })
        }
    }

    pub(super) fn expand_excerpt(
        &mut self,
        excerpt_anchor: Anchor,
        direction: ExpandExcerptDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lines_to_expand = EditorSettings::get_global(cx).expand_excerpt_lines;

        if self.delegate_expand_excerpts {
            cx.emit(EditorEvent::ExpandExcerptsRequested {
                excerpt_anchors: vec![excerpt_anchor],
                lines: lines_to_expand,
                direction,
            });
            return;
        }

        let current_scroll_position = self.scroll_position(cx);
        let mut scroll = None;

        if direction == ExpandExcerptDirection::Down {
            let multi_buffer = self.buffer.read(cx);
            let snapshot = multi_buffer.snapshot(cx);
            if let Some((buffer_snapshot, excerpt_range)) =
                snapshot.excerpt_containing(excerpt_anchor..excerpt_anchor)
            {
                let excerpt_end_row =
                    Point::from_anchor(&excerpt_range.context.end, &buffer_snapshot).row;
                let last_row = buffer_snapshot.max_point().row;
                let lines_below = last_row.saturating_sub(excerpt_end_row);
                if lines_below >= lines_to_expand {
                    scroll = Some(
                        current_scroll_position
                            + gpui::Point::new(0.0, lines_to_expand as ScrollOffset),
                    );
                }
            }
        }
        if direction == ExpandExcerptDirection::Up
            && self
                .buffer
                .read(cx)
                .snapshot(cx)
                .excerpt_before(excerpt_anchor)
                .is_none()
        {
            scroll = Some(current_scroll_position);
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.expand_excerpts([excerpt_anchor], lines_to_expand, direction, cx)
        });

        if let Some(new_scroll_position) = scroll {
            self.set_scroll_position(new_scroll_position, window, cx);
        }
    }

    pub(super) fn go_to_next_change(
        &mut self,
        _: &GoToNextChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selections) = self
            .change_list
            .next_change(1, Direction::Next)
            .map(|s| s.to_vec())
        {
            self.change_selections(Default::default(), window, cx, |s| {
                let map = s.display_snapshot();
                s.select_display_ranges(selections.iter().map(|a| {
                    let point = a.to_display_point(&map);
                    point..point
                }))
            })
        }
    }

    pub(super) fn go_to_previous_change(
        &mut self,
        _: &GoToPreviousChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selections) = self
            .change_list
            .next_change(1, Direction::Prev)
            .map(|s| s.to_vec())
        {
            self.change_selections(Default::default(), window, cx, |s| {
                let map = s.display_snapshot();
                s.select_display_ranges(selections.iter().map(|a| {
                    let point = a.to_display_point(&map);
                    point..point
                }))
            })
        }
    }

    pub(super) fn go_to_line<T: 'static>(
        &mut self,
        position: Anchor,
        highlight_color: fn(&App) -> Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.snapshot(window, cx).display_snapshot;
        let position = position.to_point(&snapshot.buffer_snapshot());
        let start = snapshot
            .buffer_snapshot()
            .clip_point(Point::new(position.row, 0), Bias::Left);
        let end = start + Point::new(1, 0);
        let start = snapshot.buffer_snapshot().anchor_before(start);
        let end = snapshot.buffer_snapshot().anchor_before(end);

        self.highlight_rows::<T>(start..end, highlight_color, Default::default(), cx);

        if self.buffer.read(cx).is_singleton() {
            self.request_autoscroll(Autoscroll::center().for_anchor(start), cx);
        }
    }

    pub fn containing_item(
        workspace: &Workspace,
        editor_id: EntityId,
        cx: &App,
    ) -> Option<Box<dyn workspace::item::ItemHandle>> {
        workspace
            .items(cx)
            .find(|item| {
                item.act_as::<Editor>(cx)
                    .is_some_and(|editor| editor.entity_id() == editor_id)
                    || item.act_as::<SplittableEditor>(cx).is_some_and(|item| {
                        let item = item.read(cx);
                        item.rhs_editor().entity_id() == editor_id
                            || item
                                .lhs_editor()
                                .is_some_and(|editor| editor.entity_id() == editor_id)
                    })
            })
            .cloned()
    }

    pub fn navigate_to_hover_links(
        &mut self,
        kind: Option<GotoDefinitionKind>,
        definitions: Vec<HoverLink>,
        origin: Option<NavigationEntry>,
        split: bool,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Task<Result<Navigated>> {
        let request = self.navigation_request();
        // Separate out url and file links, we can only handle one of them at most or an arbitrary number of locations
        let mut first_url_or_file = None;
        let definitions: Vec<_> = definitions
            .into_iter()
            .filter_map(|def| match def {
                HoverLink::Text(link) if self.lsp_data_enabled() => {
                    Some(Task::ready(anyhow::Ok(Some(link.target))))
                }
                HoverLink::LspLocation(lsp_location, server_id) if self.lsp_data_enabled() => {
                    let computation =
                        self.compute_target_location(lsp_location, server_id, window, cx);
                    Some(cx.background_spawn(computation))
                }
                HoverLink::LspUrl(url) if self.lsp_data_enabled() => {
                    first_url_or_file = Some(Either::Left((url, true)));
                    None
                }
                HoverLink::Text(_) | HoverLink::LspLocation(_, _) | HoverLink::LspUrl(_) => None,
                HoverLink::Url(url) => {
                    first_url_or_file = Some(Either::Left((url, false)));
                    None
                }
                HoverLink::File(file_target) => {
                    first_url_or_file = Some(Either::Right(file_target));
                    None
                }
            })
            .collect();

        let workspace = self.workspace().map(|workspace| workspace.downgrade());

        let excerpt_context_lines = multi_buffer::excerpt_context_lines(cx);
        cx.spawn_in(window, async move |editor, cx| {
            let locations = future::join_all(definitions).await;
            if !editor
                .read_with(cx, |source, cx| {
                    request.is_current(source)
                        && workspace.as_ref().is_none_or(|workspace| {
                            workspace.upgrade().is_some_and(|workspace| {
                                Self::containing_item(workspace.read(cx), editor.entity_id(), cx)
                                    .is_some()
                            })
                        })
                })
                .unwrap_or(false)
            {
                return Ok(Navigated::No);
            }
            let locations = if locations.is_empty()
                || editor
                    .read_with(cx, |editor, _| editor.lsp_data_enabled())
                    .unwrap_or(false)
            {
                locations
                    .into_iter()
                    .filter_map(|location| location.transpose())
                    .collect::<Result<Vec<Location>>>()
                    .context("location tasks")?
            } else {
                Vec::new()
            };
            let Ok(mut locations) = cx.update(|_, cx| {
                locations
                    .into_iter()
                    .map(|location| {
                        let buffer = location.buffer.read(cx);
                        (location.buffer, location.range.to_point(buffer))
                    })
                    .into_group_map()
            }) else {
                return Ok(Navigated::No);
            };
            let mut num_locations = 0;
            for ranges in locations.values_mut() {
                ranges.sort_unstable_by_key(|range| (range.start, Reverse(range.end)));
                ranges.dedup();
                // Merge overlapping or contained ranges. After sorting by
                // (start, Reverse(end)), we can merge in a single pass:
                // if the next range starts before the current one ends,
                // extend the current range's end if needed.
                let mut i = 0;
                while i + 1 < ranges.len() {
                    if ranges[i + 1].start <= ranges[i].end {
                        let merged_end = ranges[i].end.max(ranges[i + 1].end);
                        ranges[i].end = merged_end;
                        ranges.remove(i + 1);
                    } else {
                        i += 1;
                    }
                }
                let fits_in_one_excerpt = ranges
                    .iter()
                    .tuple_windows()
                    .all(|(a, b)| b.start.row - a.end.row <= 2 * excerpt_context_lines);
                num_locations += if fits_in_one_excerpt { 1 } else { ranges.len() };
            }

            if num_locations > 1 {
                let tab_kind = match kind {
                    Some(GotoDefinitionKind::Implementation) => "Implementations",
                    Some(GotoDefinitionKind::Symbol) | None => "Definitions",
                    Some(GotoDefinitionKind::Declaration) => "Declarations",
                    Some(GotoDefinitionKind::Type) => "Types",
                };
                let Ok(title) = editor.update_in(cx, |_, _, cx| {
                    let target = locations
                        .iter()
                        .flat_map(|(k, v)| iter::repeat(k.clone()).zip(v))
                        .map(|(buffer, location)| {
                            buffer
                                .read(cx)
                                .text_for_range(location.clone())
                                .collect::<String>()
                        })
                        .filter(|text| !text.contains('\n'))
                        .unique()
                        .take(3)
                        .join(", ");
                    if target.is_empty() {
                        tab_kind.to_owned()
                    } else {
                        format!("{tab_kind} for {target}")
                    }
                }) else {
                    return Ok(Navigated::No);
                };

                let Some(workspace) = workspace else {
                    return Ok(Navigated::No);
                };

                let opened = workspace
                    .update_in(cx, |workspace, window, cx| {
                        if editor.upgrade().is_none_or(|editor| {
                            let editor = editor.read(cx);
                            !editor.lsp_data_enabled() || !request.is_current(editor)
                        }) {
                            return false;
                        }
                        let Some(source_item) =
                            Self::containing_item(workspace, editor.entity_id(), cx)
                        else {
                            return false;
                        };
                        let Some(source_pane) = workspace.pane_for(source_item.as_ref()) else {
                            return false;
                        };
                        let allow_preview = PreviewTabsSettings::get_global(cx)
                            .enable_preview_multibuffer_from_code_navigation;
                        if let Some((target_editor, target_pane)) =
                            Self::open_locations_in_multibuffer_from_pane(
                                workspace,
                                source_pane,
                                locations,
                                title,
                                split,
                                allow_preview,
                                MultibufferSelectionMode::First,
                                window,
                                cx,
                            )
                        {
                            // We create our own nav history instead of using
                            // `target_editor.nav_history` because `nav_history`
                            // seems to be populated asynchronously when an item
                            // is added to a pane
                            let mut nav_history = target_pane
                                .update(cx, |pane, _| pane.nav_history_for_item(&target_editor));
                            target_editor.update(cx, |editor, cx| {
                                let nav_data = editor
                                    .navigation_data(editor.selections.newest_anchor().head(), cx);
                                let target =
                                    Some(nav_history.navigation_entry(Some(
                                        Arc::new(nav_data) as Arc<dyn Any + Send + Sync>
                                    )));
                                nav_history.push_tag(origin, target);
                            })
                        }
                        true
                    })
                    .unwrap_or(false);

                anyhow::Ok(Navigated::from_bool(opened))
            } else if num_locations == 0 {
                // If there is one url or file, open it directly
                match first_url_or_file {
                    Some(Either::Left((url, semantic))) => Ok(cx
                        .update(|window, cx| {
                            let Some(editor) = editor.upgrade() else {
                                return Navigated::No;
                            };
                            let editor = editor.read(cx);
                            if !request.is_current(editor)
                                || (semantic && !editor.lsp_data_enabled())
                            {
                                return Navigated::No;
                            }
                            if parse_zed_link(&url, cx).is_some() {
                                window.dispatch_action(
                                    Box::new(zed_actions::OpenZedUrl { url: url.into() }),
                                    cx,
                                );
                            } else {
                                cx.open_url(&url);
                            }
                            Navigated::Yes
                        })
                        .unwrap_or(Navigated::No)),
                    Some(Either::Right(file_target)) => {
                        // TODO(andrew): respect preview tab settings
                        //               `enable_keep_preview_on_code_navigation` and
                        //               `enable_preview_file_from_code_navigation`
                        let Some(workspace) = workspace else {
                            return Ok(Navigated::No);
                        };
                        let Ok(open) = workspace.update_in(cx, |workspace, window, cx| {
                            workspace.open_resolved_path_with_guard(
                                file_target.resolved_path.clone(),
                                window,
                                cx,
                                {
                                    let editor = editor.clone();
                                    let request = request.clone();
                                    move |workspace, cx| {
                                        editor.upgrade().is_some_and(|editor| {
                                            request.is_current(editor.read(cx))
                                                && Self::containing_item(
                                                    workspace,
                                                    editor.entity_id(),
                                                    cx,
                                                )
                                                .is_some()
                                        })
                                    }
                                },
                            )
                        }) else {
                            return Ok(Navigated::No);
                        };
                        let Some(item) = open.await? else {
                            return Ok(Navigated::No);
                        };
                        if editor
                            .read_with(cx, |editor, _| request.is_current(editor))
                            .unwrap_or(false)
                        {
                            file_target.navigate_item_to_position(item, cx);
                        }

                        Ok(Navigated::Yes)
                    }
                    None => Ok(Navigated::No),
                }
            } else {
                let Some((target_buffer, target_ranges)) = locations.into_iter().next() else {
                    return Ok(Navigated::No);
                };

                Ok(cx
                    .update(|window, cx| {
                        let Some(source_editor) = editor.upgrade() else {
                            return Navigated::No;
                        };
                        if !source_editor.read(cx).lsp_data_enabled()
                            || !request.is_current(source_editor.read(cx))
                        {
                            return Navigated::No;
                        }
                        let workspace = match workspace {
                            Some(workspace) => {
                                let Some(workspace) = workspace.upgrade() else {
                                    return Navigated::No;
                                };
                                Some(workspace)
                            }
                            None => None,
                        };
                        let source_item = if let Some(workspace) = &workspace {
                            let Some(item) = Self::containing_item(
                                workspace.read(cx),
                                source_editor.entity_id(),
                                cx,
                            ) else {
                                return Navigated::No;
                            };
                            Some(item)
                        } else {
                            None
                        };
                        let (target_ranges, same_buffer, offset) =
                            source_editor.update(cx, |editor, cx| {
                                (
                                    target_ranges
                                        .into_iter()
                                        .map(|range| editor.range_for_match(&range))
                                        .map(collapse_multiline_range)
                                        .collect::<Vec<_>>(),
                                    Some(&target_buffer)
                                        == editor.buffer.read(cx).as_singleton().as_ref(),
                                    editor.cursor_top_offset(cx),
                                )
                            });
                        if !split && same_buffer {
                            let navigated = source_editor.update(cx, |editor, cx| {
                                let multibuffer = editor.buffer.read(cx);
                                let target_ranges = target_ranges
                                    .into_iter()
                                    .filter_map(|range| {
                                        let start = multibuffer.buffer_point_to_anchor(
                                            &target_buffer,
                                            range.start,
                                            cx,
                                        )?;
                                        let end = multibuffer.buffer_point_to_anchor(
                                            &target_buffer,
                                            range.end,
                                            cx,
                                        )?;
                                        Some(start..end)
                                    })
                                    .collect::<Vec<_>>();
                                if target_ranges.is_empty() {
                                    return Navigated::No;
                                }
                                editor.change_selections(
                                    SelectionEffects::scroll(Autoscroll::for_go_to_definition(
                                        offset, cx,
                                    ))
                                    .nav_history(true),
                                    window,
                                    cx,
                                    |selections| selections.select_anchor_ranges(target_ranges),
                                );
                                let target = editor
                                    .navigation_entry(editor.selections.newest_anchor().head(), cx);
                                if let Some(mut nav_history) = editor.nav_history.clone() {
                                    nav_history.push_tag(origin, target);
                                }
                                Navigated::Yes
                            });
                            if navigated == Navigated::Yes {
                                if let Some((workspace, source_item)) =
                                    workspace.as_ref().zip(source_item.as_ref())
                                {
                                    workspace.update(cx, |workspace, cx| {
                                        workspace.activate_item(
                                            source_item.as_ref(),
                                            true,
                                            false,
                                            window,
                                            cx,
                                        );
                                    });
                                }
                                window.focus(&source_editor.focus_handle(cx), cx);
                            }
                            navigated
                        } else {
                            let Some((workspace, source_item)) = workspace.zip(source_item) else {
                                return Navigated::No;
                            };
                            let Some(pane) = workspace.read(cx).pane_for(source_item.as_ref())
                            else {
                                return Navigated::No;
                            };
                            let (target_editor, target_pane): (Entity<Self>, Entity<Pane>) =
                                workspace.update(cx, |workspace, cx| {
                                    let pane = if split {
                                        workspace.adjacent_pane_of(&pane, window, cx)
                                    } else {
                                        pane
                                    };
                                    let preview_tabs_settings = PreviewTabsSettings::get_global(cx);
                                    let keep_old_preview = preview_tabs_settings
                                        .enable_keep_preview_on_code_navigation;
                                    let allow_new_preview = preview_tabs_settings
                                        .enable_preview_file_from_code_navigation;
                                    let editor = workspace.open_project_item(
                                        pane.clone(),
                                        target_buffer.clone(),
                                        true,
                                        true,
                                        keep_old_preview,
                                        allow_new_preview,
                                        window,
                                        cx,
                                    );
                                    (editor, pane)
                                });
                            let mut nav_history = target_pane
                                .update(cx, |pane, _| pane.nav_history_for_item(&target_editor));
                            target_editor.update(cx, |target_editor, cx| {
                                let multibuffer = target_editor.buffer.read(cx);
                                let Some(target_buffer) = multibuffer.as_singleton() else {
                                    return Navigated::No;
                                };
                                let target_ranges = target_ranges
                                    .into_iter()
                                    .filter_map(|range| {
                                        let start = multibuffer.buffer_point_to_anchor(
                                            &target_buffer,
                                            range.start,
                                            cx,
                                        )?;
                                        let end = multibuffer.buffer_point_to_anchor(
                                            &target_buffer,
                                            range.end,
                                            cx,
                                        )?;
                                        Some(start..end)
                                    })
                                    .collect::<Vec<_>>();
                                if target_ranges.is_empty() {
                                    return Navigated::No;
                                }
                                target_pane.update(cx, |pane, _| pane.disable_history());
                                target_editor.change_selections(
                                    SelectionEffects::scroll(Autoscroll::for_go_to_definition(
                                        offset, cx,
                                    ))
                                    .nav_history(true),
                                    window,
                                    cx,
                                    |selections| selections.select_anchor_ranges(target_ranges),
                                );
                                let nav_data = target_editor.navigation_data(
                                    target_editor.selections.newest_anchor().head(),
                                    cx,
                                );
                                let target =
                                    Some(nav_history.navigation_entry(Some(
                                        Arc::new(nav_data) as Arc<dyn Any + Send + Sync>
                                    )));
                                nav_history.push_tag(origin, target);
                                target_pane.update(cx, |pane, _| pane.enable_history());
                                Navigated::Yes
                            })
                        }
                    })
                    .unwrap_or(Navigated::No))
            }
        })
    }

    pub(super) fn go_to_next_reference(
        &mut self,
        _: &GoToNextReference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let task = self.go_to_reference_before_or_after_position(Direction::Next, 1, window, cx);
        if let Some(task) = task {
            self.run_navigation_task(task, cx);
        }
    }

    pub(super) fn go_to_prev_reference(
        &mut self,
        _: &GoToPreviousReference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let task = self.go_to_reference_before_or_after_position(Direction::Prev, 1, window, cx);
        if let Some(task) = task {
            self.run_navigation_task(task, cx);
        }
    }

    pub(super) fn go_to_symbol_by_offset(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        offset: i8,
    ) -> Task<Result<()>> {
        let editor_snapshot = self.snapshot(window, cx);

        // We don't care about multi-buffer symbols
        if !editor_snapshot.is_singleton() {
            return Task::ready(Ok(()));
        }

        let cursor_offset = self
            .selections
            .newest::<MultiBufferOffset>(&editor_snapshot.display_snapshot)
            .head();

        cx.spawn_in(window, async move |editor, wcx| -> Result<()> {
            let Ok(Some(remote_id)) = editor.update(wcx, |ed, cx| {
                let buffer = ed.buffer.read(cx).as_singleton()?;
                Some(buffer.read(cx).remote_id())
            }) else {
                return Ok(());
            };

            let task = editor.update(wcx, |ed, cx| ed.buffer_outline_items(remote_id, cx))?;
            let outline_items: Vec<OutlineItem<text::Anchor>> = task.await;

            let multi_snapshot = editor_snapshot.buffer();
            let buffer_range = |range: &Range<_>| {
                Some(
                    multi_snapshot
                        .buffer_anchor_range_to_anchor_range(range.clone())?
                        .to_offset(multi_snapshot),
                )
            };

            wcx.update_window(wcx.window_handle(), |_, window, acx| {
                let current_idx = outline_items
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, item)| {
                        // Find the closest outline item by distance between outline text and cursor location
                        let source_range = buffer_range(&item.source_range_for_text)?;
                        let distance_to_closest_endpoint = cmp::min(
                            (source_range.start.0 as isize - cursor_offset.0 as isize).abs(),
                            (source_range.end.0 as isize - cursor_offset.0 as isize).abs(),
                        );

                        let item_towards_offset =
                            (source_range.start.0 as isize - cursor_offset.0 as isize).signum()
                                == (offset as isize).signum();

                        let source_range_contains_cursor = source_range.contains(&cursor_offset);

                        // To pick the next outline to jump to, we should jump in the direction of the offset, and
                        // we should not already be within the outline's source range. We then pick the closest outline
                        // item.
                        (item_towards_offset && !source_range_contains_cursor)
                            .then_some((distance_to_closest_endpoint, idx))
                    })
                    .min()
                    .map(|(_, idx)| idx);

                let Some(idx) = current_idx else {
                    return;
                };

                let Some(range) = buffer_range(&outline_items[idx].source_range_for_text) else {
                    return;
                };
                let selection = [range.start..range.start];

                editor
                    .update(acx, |editor, ecx| {
                        editor.change_selections(
                            SelectionEffects::scroll(Autoscroll::newest()),
                            window,
                            ecx,
                            |s| s.select_ranges(selection),
                        );
                    })
                    .log_err();
            })?;

            Ok(())
        })
    }

    pub(super) fn go_to_next_symbol(
        &mut self,
        _: &GoToNextSymbol,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_symbol_by_offset(window, cx, 1).detach();
    }

    pub(super) fn go_to_previous_symbol(
        &mut self,
        _: &GoToPreviousSymbol,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_to_symbol_by_offset(window, cx, -1).detach();
    }

    /// Opens `location` and jumps to it through the same path as
    /// go-to-definition, so selection, autoscroll, and jumplist tagging all
    /// match. `split` opens it in the adjacent pane. Called on the editor the
    /// jump originates from.
    pub fn open_location(
        &mut self,
        location: Location,
        split: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        let origin = self.navigation_entry(self.selections.newest_anchor().head(), cx);
        let link = HoverLink::Text(LocationLink {
            origin: None,
            target: location,
        });
        self.navigate_to_hover_links(None, vec![link], origin, split, window, cx)
    }

    /// Opens a multibuffer with the given project locations in it.
    pub(super) fn open_locations_in_multibuffer(
        workspace: &mut Workspace,
        locations: std::collections::HashMap<Entity<Buffer>, Vec<Range<Point>>>,
        title: String,
        split: bool,
        allow_preview: bool,
        multibuffer_selection_mode: MultibufferSelectionMode,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Option<(Entity<Editor>, Entity<Pane>)> {
        let source_pane = workspace.active_pane().clone();
        Self::open_locations_in_multibuffer_from_pane(
            workspace,
            source_pane,
            locations,
            title,
            split,
            allow_preview,
            multibuffer_selection_mode,
            window,
            cx,
        )
    }

    fn open_locations_in_multibuffer_from_pane(
        workspace: &mut Workspace,
        source_pane: Entity<Pane>,
        locations: std::collections::HashMap<Entity<Buffer>, Vec<Range<Point>>>,
        title: String,
        split: bool,
        allow_preview: bool,
        multibuffer_selection_mode: MultibufferSelectionMode,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Option<(Entity<Editor>, Entity<Pane>)> {
        if locations.is_empty() {
            log::error!("bug: open_locations_in_multibuffer called with empty list of locations");
            return None;
        }

        let capability = workspace.project().read(cx).capability();
        let mut ranges = <Vec<Range<Anchor>>>::new();

        // a key to find existing multibuffer editors with the same set of locations
        // to prevent us from opening more and more multibuffer tabs for searches and the like
        let mut key = (title.clone(), vec![]);
        let excerpt_buffer = cx.new(|cx| {
            let key = &mut key.1;
            let mut multibuffer = MultiBuffer::new(capability);
            for (buffer, mut ranges_for_buffer) in locations {
                ranges_for_buffer.sort_by_key(|range| (range.start, Reverse(range.end)));
                key.push((buffer.read(cx).remote_id(), ranges_for_buffer.clone()));
                multibuffer.set_excerpts_for_path(
                    PathKey::for_buffer(&buffer, cx),
                    buffer.clone(),
                    ranges_for_buffer.clone(),
                    multibuffer_context_lines(cx),
                    cx,
                );
                let snapshot = multibuffer.snapshot(cx);
                let buffer_snapshot = buffer.read(cx).snapshot();
                ranges.extend(ranges_for_buffer.into_iter().filter_map(|range| {
                    let text_range = buffer_snapshot.anchor_range_inside(range);
                    let start = snapshot.anchor_in_buffer(text_range.start)?;
                    let end = snapshot.anchor_in_buffer(text_range.end)?;
                    Some(start..end)
                }))
            }

            let final_snapshot = multibuffer.snapshot(cx);
            ranges.sort_by(|a, b| a.start.cmp(&b.start, &final_snapshot));

            multibuffer.with_title(title)
        });
        let pane = if split {
            workspace.adjacent_pane_of(&source_pane, window, cx)
        } else {
            source_pane
        };
        let activate_pane = split || &pane != workspace.active_pane();
        let existing = pane.update(cx, |pane, cx| {
            pane.items()
                .filter_map(|item| item.downcast::<Editor>())
                .find(|editor| {
                    editor
                        .read(cx)
                        .lookup_key
                        .as_ref()
                        .and_then(|it| {
                            it.downcast_ref::<(String, Vec<(BufferId, Vec<Range<Point>>)>)>()
                        })
                        .is_some_and(|it| *it == key)
                })
        });
        let was_existing = existing.is_some();
        let editor = existing.unwrap_or_else(|| {
            cx.new(|cx| {
                let mut editor = Editor::for_multibuffer(
                    excerpt_buffer,
                    Some(workspace.project().clone()),
                    window,
                    cx,
                );
                editor.lookup_key = Some(Box::new(key));
                editor
            })
        });
        editor.update(cx, |editor, cx| match multibuffer_selection_mode {
            MultibufferSelectionMode::First => {
                if let Some(first_range) = ranges.first() {
                    editor.change_selections(
                        SelectionEffects::no_scroll(),
                        window,
                        cx,
                        |selections| {
                            selections.clear_disjoint();
                            selections.select_anchor_ranges(std::iter::once(first_range.clone()));
                        },
                    );
                }
                editor.highlight_background(
                    HighlightKey::Editor,
                    &ranges,
                    |_, theme| theme.colors().editor_highlighted_line_background,
                    cx,
                );
            }
            MultibufferSelectionMode::All => {
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.clear_disjoint();
                    selections.select_anchor_ranges(ranges);
                });
            }
        });

        let item = Box::new(editor.clone());

        let mut destination_index = None;
        pane.update(cx, |pane, cx| {
            if allow_preview && !was_existing {
                destination_index = pane.replace_preview_item_id(item.item_id(), window, cx);
                editor.update(cx, |editor, cx| {
                    editor
                        .buffer
                        .update(cx, |buffer, cx| buffer.refresh_preview(cx))
                });
            }
            if was_existing && !allow_preview {
                pane.unpreview_item_if_preview(item.item_id());
            }
            pane.add_item(item, activate_pane, true, destination_index, window, cx);
        });

        Some((editor, pane))
    }

    fn navigation_data(&self, cursor_anchor: Anchor, cx: &mut Context<Self>) -> NavigationData {
        let display_snapshot = self.display_map.update(cx, |map, cx| map.snapshot(cx));
        let buffer = self.buffer.read(cx).read(cx);
        let cursor_position = cursor_anchor.to_point(&buffer);
        let scroll_anchor = self.scroll_manager.native_anchor(&display_snapshot, cx);
        let scroll_top_row = scroll_anchor.top_row(&buffer);
        drop(buffer);

        NavigationData {
            cursor_anchor,
            cursor_position,
            scroll_anchor,
            scroll_top_row,
        }
    }

    fn expand_excerpts_for_direction(
        &mut self,
        lines: u32,
        direction: ExpandExcerptDirection,
        cx: &mut Context<Self>,
    ) {
        let selections = self.selections.disjoint_anchors_arc();

        let lines = if lines == 0 {
            EditorSettings::get_global(cx).expand_excerpt_lines
        } else {
            lines
        };

        let snapshot = self.buffer.read(cx).snapshot(cx);
        let excerpt_anchors = selections
            .iter()
            .flat_map(|selection| {
                snapshot
                    .range_to_buffer_ranges(selection.range())
                    .into_iter()
                    .filter_map(|(buffer_snapshot, range, _)| {
                        snapshot.anchor_in_excerpt(buffer_snapshot.anchor_after(range.start))
                    })
            })
            .collect::<Vec<_>>();

        if self.delegate_expand_excerpts {
            cx.emit(EditorEvent::ExpandExcerptsRequested {
                excerpt_anchors,
                lines,
                direction,
            });
            return;
        }

        self.buffer.update(cx, |buffer, cx| {
            buffer.expand_excerpts(excerpt_anchors, lines, direction, cx)
        })
    }

    pub(crate) fn go_to_definition_of_kind(
        &mut self,
        kind: GotoDefinitionKind,
        split: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        self.begin_navigation();
        self.go_to_definition_for_request(kind, split, window, cx)
    }

    pub(crate) fn go_to_definition_for_request(
        &mut self,
        kind: GotoDefinitionKind,
        split: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Navigated>> {
        if !self.lsp_data_enabled() {
            return Task::ready(Ok(Navigated::No));
        }
        let request = self.navigation_request();
        let Some(provider) = self.semantics_provider.clone() else {
            return Task::ready(Ok(Navigated::No));
        };
        let head = self
            .selections
            .newest::<MultiBufferOffset>(&self.display_snapshot(cx))
            .head();
        let buffer = self.buffer.read(cx);
        let Some((buffer, head)) = buffer.text_anchor_for_position(head, cx) else {
            return Task::ready(Ok(Navigated::No));
        };
        let Some(definitions) = provider.definitions(&buffer, head, kind, cx) else {
            return Task::ready(Ok(Navigated::No));
        };

        let nav_entry = self.navigation_entry(self.selections.newest_anchor().head(), cx);

        cx.spawn_in(window, async move |editor, cx| {
            let definitions = definitions.await;
            let Ok(true) = editor.read_with(cx, |editor, _| {
                editor.lsp_data_enabled() && request.is_current(editor)
            }) else {
                return Ok(Navigated::No);
            };
            let Some(definitions) = definitions? else {
                return Ok(Navigated::No);
            };
            let Ok(navigation) = editor.update_in(cx, |editor, window, cx| {
                editor.navigate_to_hover_links(
                    Some(kind),
                    definitions
                        .into_iter()
                        .filter(|location| {
                            hover_links::exclude_link_to_position(&buffer, &head, location, cx)
                        })
                        .map(HoverLink::Text)
                        .collect::<Vec<_>>(),
                    nav_entry,
                    split,
                    window,
                    cx,
                )
            }) else {
                return Ok(Navigated::No);
            };
            navigation.await
        })
    }

    fn compute_target_location(
        &self,
        lsp_location: lsp::Location,
        server_id: LanguageServerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<Option<Location>>> {
        let request = self.navigation_request();
        let Some(project) = self.project.clone() else {
            return Task::ready(Ok(None));
        };

        cx.spawn_in(window, async move |editor, cx| {
            let Ok(Some(location_task)) = editor.update(cx, |editor, cx| {
                if !editor.lsp_data_enabled() || !request.is_current(editor) {
                    return None;
                }
                Some(project.update(cx, |project, cx| {
                    project.open_local_buffer_via_lsp(lsp_location.uri.clone(), server_id, cx)
                }))
            }) else {
                return Ok(None);
            };
            let target_buffer_handle = location_task.await;
            let Ok(true) = editor.read_with(cx, |editor, _| {
                editor.lsp_data_enabled() && request.is_current(editor)
            }) else {
                return Ok(None);
            };
            let location = Some({
                let target_buffer_handle = target_buffer_handle.context("open local buffer")?;
                let range = target_buffer_handle.read_with(cx, |target_buffer, _| {
                    let target_start = target_buffer
                        .clip_point_utf16(point_from_lsp(lsp_location.range.start), Bias::Left);
                    let target_end = target_buffer
                        .clip_point_utf16(point_from_lsp(lsp_location.range.end), Bias::Left);
                    target_buffer.anchor_after(target_start)
                        ..target_buffer.anchor_before(target_end)
                });
                Location {
                    buffer: target_buffer_handle,
                    range,
                }
            });
            Ok(location)
        })
    }

    fn go_to_singleton_buffer_range_impl(
        &mut self,
        range: Range<Point>,
        record_nav_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let multibuffer = self.buffer().read(cx);
        let Some(buffer) = multibuffer.as_singleton() else {
            return;
        };
        let Some(start) = multibuffer.buffer_point_to_anchor(&buffer, range.start, cx) else {
            return;
        };
        let Some(end) = multibuffer.buffer_point_to_anchor(&buffer, range.end, cx) else {
            return;
        };
        self.change_selections(
            SelectionEffects::scroll(Autoscroll::for_go_to_definition(
                self.cursor_top_offset(cx),
                cx,
            ))
            .nav_history(record_nav_history),
            window,
            cx,
            |s| s.select_anchor_ranges([start..end]),
        );
    }

    fn go_to_document_highlight_before_or_after_position(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) {
        let snapshot = self.snapshot(window, cx);
        let buffer = &snapshot.buffer_snapshot();
        let position = self
            .selections
            .newest::<Point>(&snapshot.display_snapshot)
            .head();
        let anchor_position = buffer.anchor_after(position);

        // Get all document highlights (both read and write)
        let mut all_highlights = Vec::new();

        if let Some((_, read_highlights)) = self
            .background_highlights
            .get(&HighlightKey::DocumentHighlightRead)
        {
            all_highlights.extend(read_highlights.iter());
        }

        if let Some((_, write_highlights)) = self
            .background_highlights
            .get(&HighlightKey::DocumentHighlightWrite)
        {
            all_highlights.extend(write_highlights.iter());
        }

        if all_highlights.is_empty() {
            return;
        }

        // Sort highlights by position
        all_highlights.sort_by(|a, b| a.start.cmp(&b.start, buffer));

        let target_highlight = match direction {
            Direction::Next => {
                // Find the first highlight after the current position
                all_highlights
                    .iter()
                    .find(|highlight| highlight.start.cmp(&anchor_position, buffer).is_gt())
            }
            Direction::Prev => {
                // Find the last highlight before the current position
                all_highlights
                    .iter()
                    .rev()
                    .find(|highlight| highlight.end.cmp(&anchor_position, buffer).is_lt())
            }
        };

        if let Some(highlight) = target_highlight {
            let destination = highlight.start.to_point(buffer);
            let autoscroll = Autoscroll::center();

            self.unfold_ranges(&[destination..destination], false, false, cx);
            self.change_selections(SelectionEffects::scroll(autoscroll), window, cx, |s| {
                s.select_ranges([destination..destination]);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        editor_tests::init_test,
        test::{
            editor_lsp_test_context::EditorLspTestContext, editor_test_context::EditorTestContext,
        },
    };
    use buffer_diff::BufferDiff;
    use futures::{StreamExt as _, channel::oneshot};
    use gpui::{AnyEntity, TestAppContext};
    use settings::DiffViewStyle;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use workspace::{Item, SplitDirection};

    const SOURCE: &str = "fn target() {}\nfn main() { tarˇget(); }\n";
    const URL: &str = "https://example.com/navigation";

    #[gpui::test]
    async fn test_navigation_task_ownership(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for release_source in [false, true] {
            let mut editor_cx = EditorTestContext::new(cx).await;
            let dropped = Arc::new(AtomicUsize::new(0));
            let cleanups = Arc::new(AtomicUsize::new(0));
            let (resume, pending) = oneshot::channel::<()>();
            let (started_sender, started) = oneshot::channel();
            let request = editor_cx.update_editor(|editor, _, cx| {
                let request = editor.navigation_request();
                editor
                    .navigation
                    .reference_sources
                    .push((editor.selections.newest_anchor().head(), request.clone()));
                let task = cx.spawn({
                    let request = request.clone();
                    let dropped = dropped.clone();
                    let cleanups = cleanups.clone();
                    async move |editor, cx| {
                        let _drop = util::defer(move || {
                            dropped.fetch_add(1, Ordering::SeqCst);
                        });
                        let _cleanup = cx.on_drop(&editor, move |editor, _| {
                            editor
                                .navigation
                                .reference_sources
                                .retain(|(_, source_request)| source_request != &request);
                            cleanups.fetch_add(1, Ordering::SeqCst);
                        });
                        started_sender
                            .send(())
                            .expect("navigation started listener");
                        pending.await?;
                        anyhow::Ok(())
                    }
                });
                editor.run_navigation_task(task, cx);
                request
            });
            started.await.expect("navigation started");
            editor_cx.run_until_parked();
            assert_eq!(dropped.load(Ordering::SeqCst), 0);
            assert_eq!(cleanups.load(Ordering::SeqCst), 0);

            if release_source {
                let source = editor_cx.editor.downgrade();
                editor_cx.update(|window, _| window.remove_window());
                cx.update(|_| drop(editor_cx));
                cx.run_until_parked();
                source.assert_released();
                assert_eq!(dropped.load(Ordering::SeqCst), 1);
                assert_eq!(cleanups.load(Ordering::SeqCst), 0);
            } else {
                let replacement = editor_cx.update_editor(|editor, _, _| {
                    let replacement = editor.begin_navigation();
                    assert!(!request.is_current(editor));
                    assert!(replacement.is_current(editor));
                    assert_eq!(editor.navigation.reference_sources.len(), 0);
                    editor.navigation.reference_sources.push((
                        editor.selections.newest_anchor().head(),
                        replacement.clone(),
                    ));
                    replacement
                });
                editor_cx.run_until_parked();
                assert_eq!(dropped.load(Ordering::SeqCst), 1);
                assert_eq!(cleanups.load(Ordering::SeqCst), 1);
                let (resume_replacement, pending_replacement) = oneshot::channel::<()>();
                let (finished_sender, finished) = oneshot::channel();
                editor_cx.update_editor(|editor, _, cx| {
                    assert!(replacement.is_current(editor));
                    assert_eq!(editor.navigation.reference_sources.len(), 1);
                    assert!(
                        editor
                            .navigation
                            .reference_sources
                            .first()
                            .expect("replacement reference source")
                            .1
                            == replacement
                    );
                    let task = cx.spawn(async move |_, _| {
                        pending_replacement.await?;
                        finished_sender.send(()).expect("replacement listener");
                        anyhow::Ok(())
                    });
                    editor.run_navigation_task(task, cx);
                });
                editor_cx.run_until_parked();
                resume_replacement.send(()).expect("resume replacement");
                finished.await.expect("replacement completed");
                editor_cx.run_until_parked();
            }
            assert_eq!(resume.send(()), Err(()));
        }
    }

    #[gpui::test]
    async fn test_pending_location_queries_decline_ineligible_navigation(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        let mut cx = navigation_context(cx).await;
        for (references, disable) in [(false, false), (false, true), (true, false), (true, true)] {
            cx.update_editor(|editor, _, _| editor.enable_lsp_data = true);
            let pending = pending_locations(&cx, references, vec![target_location(&cx)]);
            let query = cx
                .update_editor(|editor, _, cx| {
                    if references {
                        let project = editor.project.clone().expect("project");
                        editor.find_all_references_locations(&project, cx)
                    } else {
                        editor.definition_locations_of_kind(GotoDefinitionKind::Symbol, cx)
                    }
                })
                .expect("location query");
            invalidate_response(&mut cx, pending, disable).await;
            assert_eq!(query.await.expect("location result").len(), 0);
            assert_navigation_state(&mut cx, SOURCE);
        }
    }

    #[gpui::test]
    async fn test_pending_native_navigation_declines_ineligible_navigation(
        cx: &mut TestAppContext,
    ) {
        init_test(cx, |_| {});
        let mut cx = navigation_context(cx).await;
        for (references, disable) in [(false, false), (false, true), (true, false), (true, true)] {
            for split in [false, true] {
                cx.update_editor(|editor, _, _| editor.enable_lsp_data = true);
                let pending = pending_locations(&cx, references, vec![target_location(&cx)]);
                let navigation = native_navigation(&mut cx, references, split);
                invalidate_response(&mut cx, pending, disable).await;
                assert_eq!(navigation.await.expect("navigation result"), Navigated::No);
                assert_navigation_state(&mut cx, SOURCE);
            }
        }
    }

    #[gpui::test]
    async fn test_pending_reference_steps_decline_disabled_lsp_data(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        let mut cx = navigation_context(cx).await;

        for direction in [Direction::Next, Direction::Prev] {
            cx.update_editor(|editor, _, _| editor.enable_lsp_data = true);
            let pending = pending_locations(
                &cx,
                true,
                vec![
                    target_location(&cx),
                    lsp::Location {
                        uri: cx.buffer_lsp_url.clone(),
                        range: lsp::Range::new(
                            lsp::Position::new(1, 12),
                            lsp::Position::new(1, 18),
                        ),
                    },
                ],
            );
            cx.update_editor(|editor, window, cx| {
                if direction == Direction::Next {
                    editor.go_to_next_reference(&GoToNextReference, window, cx);
                } else {
                    editor.go_to_prev_reference(&GoToPreviousReference, window, cx);
                }
                assert!(!editor.navigation.task.is_ready());
            });

            invalidate_response(&mut cx, pending, true).await;
            assert_navigation_state(&mut cx, SOURCE);
        }
    }

    #[gpui::test]
    async fn test_pending_hover_navigation_preserves_nonsemantic_links(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        let mut cx = navigation_context(cx).await;

        for file in [false, true] {
            let nonsemantic_link = if file {
                file_link(
                    project::ResolvedPath::AbsPath {
                        path: EditorLspTestContext::root_path()
                            .join("dir/file.rs")
                            .to_string_lossy()
                            .into_owned(),
                        is_dir: false,
                    },
                    (2, 1),
                )
            } else {
                HoverLink::Url(String::from(URL))
            };
            let navigation = cx.update_editor(|editor, window, cx| {
                editor.enable_lsp_data = true;
                let semantic_link = HoverLink::Text(LocationLink {
                    origin: None,
                    target: local_target(editor, cx),
                });
                let navigation = editor.navigate_to_hover_links(
                    None,
                    vec![semantic_link, nonsemantic_link],
                    None,
                    false,
                    window,
                    cx,
                );
                editor.disable_lsp_data();
                navigation
            });

            assert_eq!(navigation.await.expect("navigation result"), Navigated::Yes);
            if file {
                assert_navigation_state(&mut cx, "fn target() {}\nˇfn main() { target(); }\n");
            } else {
                assert_navigation_state(&mut cx, SOURCE);
                assert_eq!(cx.opened_url(), Some(String::from(URL)));
            }
        }
    }

    #[gpui::test]
    async fn test_pending_file_navigation_checks_load_to_open_boundary(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for (absolute, outside) in [(false, false), (true, false), (true, true)] {
            for remove_source in [None, Some(false), Some(true)] {
                let mut cx = navigation_context(cx).await;
                let resolved_path = navigation_file_target(&mut cx, absolute, outside).await;
                let project = cx.update_workspace(|workspace, _, _| workspace.project().clone());
                cx.run_until_parked();
                let source = cx.editor.clone();
                let navigation = cx.update_editor(|editor, window, cx| {
                    editor.begin_navigation();
                    editor.navigate_to_hover_links(
                        None,
                        vec![file_link(resolved_path, (2, 3))],
                        None,
                        false,
                        window,
                        cx,
                    )
                });
                while cx.read(|cx| {
                    project
                        .read(cx)
                        .buffer_store()
                        .read(cx)
                        .loading_buffers()
                        .next()
                        .is_none()
                }) {
                    assert!(cx.executor().tick(), "file load did not start");
                }
                cx.update_workspace(|workspace, _, cx| {
                    assert_eq!(workspace.items(cx).count(), 1);
                    assert_eq!(workspace.active_item_as::<Editor>(cx), Some(source.clone()));
                });
                if let Some(remove_source) = remove_source {
                    if remove_source {
                        cx.update_workspace(|workspace, window, cx| {
                            workspace.active_pane().update(cx, |pane, cx| {
                                pane.remove_item(source.entity_id(), false, false, window, cx);
                            });
                        });
                    } else {
                        cx.update_editor(|editor, _, _| {
                            editor.begin_navigation();
                        });
                    }
                }
                assert_eq!(
                    navigation.await.expect("file navigation"),
                    Navigated::from_bool(remove_source.is_none())
                );
                cx.run_until_parked();
                cx.assert_editor_state(SOURCE);
                cx.update_workspace(|workspace, _, cx| {
                    assert_eq!(workspace.panes().len(), 1);
                    if let Some(remove_source) = remove_source {
                        assert_eq!(workspace.items(cx).count(), usize::from(!remove_source));
                        assert_eq!(
                            workspace.active_item_as::<Editor>(cx),
                            (!remove_source).then_some(source)
                        );
                    } else {
                        assert_eq!(workspace.items(cx).count(), 2);
                        let target = workspace
                            .active_item_as::<Editor>(cx)
                            .expect("target editor");
                        let target = target.read(cx);
                        assert_eq!(
                            target.buffer().read(cx).snapshot(cx).text(),
                            "first\nsecond\n"
                        );
                        assert_eq!(
                            target
                                .selections
                                .newest_anchor()
                                .head()
                                .to_point(&target.buffer().read(cx).snapshot(cx)),
                            Point::new(1, 2)
                        );
                    }
                    assert_eq!(
                        workspace.project().read(cx).visible_worktrees(cx).count(),
                        1
                    );
                });
            }
        }
    }

    #[gpui::test]
    async fn test_file_navigation_reports_opened_when_superseded_after_open(
        cx: &mut TestAppContext,
    ) {
        init_test(cx, |_| {});
        for absolute in [false, true] {
            let mut cx = navigation_context(cx).await;
            let path = navigation_file_target(&mut cx, absolute, false).await;
            let workspace = cx.workspace.clone();
            let subscription = cx.update_editor(|_, window, cx| {
                cx.subscribe_in(&workspace, window, |editor, _, event, window, cx| {
                    if let workspace::Event::ItemAdded { item } = event {
                        editor.begin_navigation();
                        item.downcast::<Editor>().expect("opened editor").update(
                            cx,
                            |editor, cx| {
                                editor.go_to_singleton_buffer_point_silently(
                                    Point::new(0, 1),
                                    window,
                                    cx,
                                );
                            },
                        );
                    }
                })
            });
            let (request, navigation) = cx.update_editor(|editor, window, cx| {
                let request = editor.begin_navigation();
                let navigation = editor.navigate_to_hover_links(
                    None,
                    vec![file_link(path, (2, 3))],
                    None,
                    false,
                    window,
                    cx,
                );
                (request, navigation)
            });
            assert_eq!(navigation.await.expect("opened navigation"), Navigated::Yes);
            cx.run_until_parked();
            cx.editor(|editor, _, _| assert!(!request.is_current(editor)));
            cx.editor = cx.update_workspace(|workspace, _, cx| {
                assert_eq!(workspace.items(cx).count(), 2);
                workspace
                    .active_item_as::<Editor>(cx)
                    .expect("opened editor")
            });
            cx.assert_editor_state("fˇirst\nsecond\n");
            drop(subscription);
        }
    }

    #[gpui::test]
    async fn test_guarded_directory_navigation_preserves_resolved_path_semantics(
        cx: &mut TestAppContext,
    ) {
        init_test(cx, |_| {});
        for absolute in [false, true] {
            for (guarded, cancel) in [(false, false), (true, false), (true, true)] {
                let mut cx = navigation_context(cx).await;
                let directory = EditorLspTestContext::root_path().join("dir");
                let project = cx.update_workspace(|workspace, _, _| workspace.project().clone());
                let (project_path, entry_id) = cx.read(|cx| {
                    let project = project.read(cx);
                    let path = project
                        .find_project_path(&directory, cx)
                        .expect("directory project path");
                    let entry_id = project
                        .entry_for_path(&path, cx)
                        .expect("directory entry")
                        .id;
                    (path, entry_id)
                });
                let path = if absolute {
                    project::ResolvedPath::AbsPath {
                        path: directory.to_string_lossy().into_owned(),
                        is_dir: true,
                    }
                } else {
                    project::ResolvedPath::ProjectPath {
                        project_path,
                        is_dir: true,
                    }
                };
                let mut events = cx.events(&project);
                let checks = Arc::new(AtomicUsize::new(0));
                let result = if guarded {
                    let checks = checks.clone();
                    cx.update_workspace(|workspace, window, cx| {
                        workspace.open_resolved_path_with_guard(path, window, cx, move |_, _| {
                            let first_check = checks.fetch_add(1, Ordering::SeqCst) == 0;
                            first_check && !cancel
                        })
                    })
                    .await
                } else {
                    cx.update_workspace(|workspace, window, cx| {
                        workspace.open_resolved_path(path, window, cx)
                    })
                    .await
                    .map(Some)
                };
                if cancel {
                    assert!(result.expect("cancelled directory navigation").is_none());
                } else {
                    assert!(result.is_err());
                }
                assert_navigation_state(&mut cx, SOURCE);
                assert_eq!(checks.load(Ordering::SeqCst), usize::from(guarded));
                let mut revealed = Vec::new();
                while let Some(Some(event)) = events.next().now_or_never() {
                    if let project::Event::ActiveEntryChanged(entry) = event {
                        revealed.push(entry);
                    }
                }
                assert_eq!(
                    revealed,
                    if absolute && !cancel {
                        vec![Some(entry_id)]
                    } else {
                        Vec::new()
                    }
                );
            }
        }
    }

    #[gpui::test]
    async fn test_late_split_lhs_file_navigation(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        let mut cx = navigation_context(cx).await;
        let wrapper = cx.update_workspace(|workspace, window, cx| {
            let multibuffer = cx.new(|_| MultiBuffer::new(Capability::ReadWrite));
            let project = workspace.project().clone();
            let workspace_handle = cx.entity();
            let wrapper = cx.new(|cx| {
                SplittableEditor::new(
                    DiffViewStyle::Unified,
                    multibuffer,
                    project,
                    workspace_handle,
                    window,
                    cx,
                )
            });
            workspace.active_pane().update(cx, |pane, cx| {
                pane.add_item(Box::new(wrapper.clone()), true, true, None, window, cx);
            });
            wrapper
        });
        cx.run_until_parked();
        for added_before_defer in [false, true] {
            let workspace = cx.workspace.clone();
            let marker = Some(workspace::WorkspaceId::from_i64(-1));
            let lhs = cx.update(|window, cx| {
                wrapper.update(cx, |wrapper, cx| {
                    wrapper.toggle_split(&crate::split::ToggleSplitDiff, window, cx);
                });
                let lhs = wrapper.read(cx).lhs_editor().expect("left editor").clone();
                if added_before_defer {
                    workspace.update(cx, |workspace, cx| {
                        wrapper.update(cx, |wrapper, cx| {
                            wrapper.added_to_workspace(workspace, window, cx);
                        })
                    });
                    lhs.update(cx, |editor, _| {
                        editor.workspace.as_mut().expect("workspace").1 = marker
                    });
                }
                lhs
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                lhs.update(cx, |editor, _| {
                    if added_before_defer {
                        assert_eq!(editor.workspace.as_ref().expect("workspace").1, marker);
                    }
                    editor.workspace.as_mut().expect("workspace").1 = marker;
                });
                workspace.update(cx, |workspace, cx| {
                    wrapper.update(cx, |wrapper, cx| {
                        wrapper.added_to_workspace(workspace, window, cx);
                    })
                });
                assert_eq!(
                    lhs.read(cx).workspace.as_ref().expect("workspace").1,
                    marker
                );
            });
            let navigation = cx.update(|window, cx| {
                lhs.update(cx, |editor, cx| {
                    assert_eq!(editor.workspace(), Some(workspace));
                    assert!(!editor.lsp_data_enabled());
                    editor.navigate_to_hover_links(
                        None,
                        vec![file_link(
                            project::ResolvedPath::AbsPath {
                                path: EditorLspTestContext::root_path()
                                    .join("dir/file.rs")
                                    .to_string_lossy()
                                    .into_owned(),
                                is_dir: false,
                            },
                            (2, 1),
                        )],
                        None,
                        false,
                        window,
                        cx,
                    )
                })
            });
            assert_eq!(
                navigation.await.expect("left file navigation"),
                Navigated::Yes
            );
            cx.assert_editor_state("fn target() {}\nˇfn main() { target(); }\n");
            cx.update(|window, cx| {
                wrapper.update(cx, |wrapper, cx| {
                    wrapper.toggle_split(&crate::split::ToggleSplitDiff, window, cx)
                })
            });
        }
    }

    #[gpui::test]
    async fn test_pending_lsp_url_declines_disabled_lsp_data(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for semantic in [true, false] {
            for disable_before in [false, true] {
                let mut cx = navigation_context(cx).await;
                let navigation = cx.update_editor(|editor, window, cx| {
                    if disable_before {
                        editor.disable_lsp_data();
                    }
                    let url = String::from(URL);
                    let link = if semantic {
                        HoverLink::LspUrl(url)
                    } else {
                        HoverLink::Url(url)
                    };
                    let navigation =
                        editor.navigate_to_hover_links(None, vec![link], None, false, window, cx);
                    editor.disable_lsp_data();
                    navigation
                });
                assert_eq!(
                    navigation.await.expect("navigation result"),
                    Navigated::from_bool(!semantic)
                );
                assert_eq!(cx.opened_url(), (!semantic).then(|| String::from(URL)));
                assert_navigation_state(&mut cx, SOURCE);
            }
        }
    }

    #[gpui::test]
    async fn test_pending_native_fallback_declines_superseded_navigation(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        cx.update(|cx| {
            let mut settings = EditorSettings::get_global(cx).clone();
            settings.go_to_definition_fallback = GoToDefinitionFallback::FindAllReferences;
            EditorSettings::override_global(settings, cx);
        });
        let mut cx = navigation_context(cx).await;
        let references = Arc::new(AtomicUsize::new(0));
        cx.set_request_handler::<lsp::request::References, _, _>({
            let references = references.clone();
            move |_, _, _| {
                references.fetch_add(1, Ordering::SeqCst);
                async move { Ok(None) }
            }
        });
        let pending = pending_response::<lsp::request::GotoDefinition>(&cx, None);
        let navigation = cx.update_editor(|editor, window, cx| {
            editor.go_to_definition(&GoToDefinition::default(), window, cx)
        });
        invalidate_response(&mut cx, pending, false).await;
        assert_eq!(navigation.await.expect("navigation result"), Navigated::No);
        cx.run_until_parked();
        assert_eq!(references.load(Ordering::SeqCst), 0);
        assert_navigation_state(&mut cx, SOURCE);

        let (started, respond) = pending_locations(&cx, true, vec![target_location(&cx)]);
        let old_navigation = native_navigation(&mut cx, true, false);
        started.await.expect("old reference request started");
        let (new_started, new_respond) = pending_locations(&cx, true, vec![target_location(&cx)]);
        let new_navigation = native_navigation(&mut cx, true, false);
        new_started.await.expect("new reference request started");
        let reservation = cx.editor(|editor, _, _| {
            assert_eq!(editor.navigation.reference_sources.len(), 1);
            editor.navigation.reference_sources[0].clone()
        });
        respond.send(()).expect("release old reference response");
        assert_eq!(
            old_navigation.await.expect("old navigation result"),
            Navigated::No
        );
        cx.run_until_parked();
        cx.update_editor(|editor, window, cx| {
            assert_eq!(editor.navigation.reference_sources.len(), 1);
            let (anchor, request) = &editor.navigation.reference_sources[0];
            assert_eq!(*anchor, reservation.0);
            assert!(*request == reservation.1);
            let query = hover_links::NavigationSource {
                editor: cx.weak_entity(),
                position: editor.selections.newest_anchor().head(),
                origin: None,
                request: request.clone(),
            };
            assert!(editor.find_all_references_at(&query, window, cx).is_none());
        });
        new_respond
            .send(())
            .expect("release new reference response");
        assert_eq!(
            new_navigation.await.expect("new navigation result"),
            Navigated::Yes
        );
        assert_navigation_state(&mut cx, "fn ˇtarget() {}\nfn main() { target(); }\n");
    }

    #[gpui::test]
    async fn test_pending_native_navigation_uses_source_pane(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for (references, split) in [(false, false), (false, true), (true, false)] {
            for (multibuffer, remove_source) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                assert_pending_native_navigation_uses_source_pane(
                    cx,
                    split,
                    multibuffer,
                    remove_source,
                    references,
                    false,
                    false,
                )
                .await;
            }
        }
        for (multibuffer, remove_source) in [(false, false), (false, true), (true, false)] {
            assert_pending_native_navigation_uses_source_pane(
                cx,
                false,
                multibuffer,
                remove_source,
                false,
                true,
                false,
            )
            .await;
        }
    }

    #[gpui::test]
    async fn test_pending_native_navigation_uses_wrapped_source_pane(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for split_editor in [false, true] {
            for (references, split) in [(false, false), (false, true), (true, false)] {
                for (multibuffer, remove_source) in
                    [(false, false), (false, true), (true, false), (true, true)]
                {
                    assert_pending_native_navigation_uses_source_pane(
                        cx,
                        split,
                        multibuffer,
                        remove_source,
                        references,
                        split_editor,
                        true,
                    )
                    .await;
                }
            }
        }
    }

    #[gpui::test]
    async fn test_native_same_buffer_navigation_without_workspace(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for split in [false, true] {
            let mut cx = EditorTestContext::new(cx).await;
            cx.set_state(SOURCE);
            let (superseded, navigation) = cx.update_editor(|editor, window, cx| {
                assert!(editor.workspace().is_none());
                let request = editor.begin_navigation();
                let superseded = editor.open_location(local_target(editor, cx), false, window, cx);
                editor.begin_navigation();
                let navigation = editor.open_location(local_target(editor, cx), split, window, cx);
                assert!(!request.is_current(editor));
                (superseded, navigation)
            });
            assert_eq!(
                superseded.await.expect("superseded navigation"),
                Navigated::No
            );
            assert_eq!(
                navigation.await.expect("navigation result"),
                Navigated::from_bool(!split)
            );
            if split {
                cx.assert_editor_state(SOURCE);
            } else {
                cx.assert_editor_state("fn «targetˇ»() {}\nfn main() { target(); }\n");
            }
        }

        for move_source in [false, true] {
            let mut cx = navigation_context(cx).await;
            let source = cx.editor.clone();
            let (source_pane, hidden_by) = cx.update_workspace(|workspace, window, cx| {
                let pane = workspace.active_pane().clone();
                let editor = cx.new(|cx| Editor::single_line(window, cx));
                pane.update(cx, |pane, cx| {
                    pane.add_item(Box::new(editor.clone()), true, true, None, window, cx);
                });
                (pane, editor)
            });
            cx.update_workspace(|workspace, _, cx| {
                assert_eq!(
                    Editor::containing_item(workspace, source.entity_id(), cx)
                        .map(|item| item.item_id()),
                    Some(source.entity_id())
                );
            });
            let navigation = cx.update_editor(|editor, window, cx| {
                editor.open_location(local_target(editor, cx), false, window, cx)
            });
            let expected_pane = if move_source {
                cx.update_workspace(|workspace, window, cx| {
                    let destination = workspace.split_pane(
                        source_pane.clone(),
                        SplitDirection::Right,
                        window,
                        cx,
                    );
                    workspace::move_item(
                        &source_pane,
                        &destination,
                        source.entity_id(),
                        0,
                        false,
                        window,
                        cx,
                    );
                    window.focus(&hidden_by.focus_handle(cx), cx);
                    destination
                })
            } else {
                source_pane
            };
            assert_eq!(navigation.await.expect("navigation result"), Navigated::Yes);
            cx.assert_editor_state("fn «targetˇ»() {}\nfn main() { target(); }\n");
            cx.update_workspace(|workspace, window, cx| {
                assert_eq!(workspace.active_pane(), &expected_pane);
                assert_eq!(workspace.active_item_as::<Editor>(cx), Some(source.clone()));
                assert!(source.focus_handle(cx).is_focused(window));
                assert_eq!(workspace.items(cx).count(), 2);
            });
        }
    }

    #[gpui::test]
    async fn test_pending_location_queries_ignore_editor_teardown(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for references in [false, true] {
            let mut cx = navigation_context(cx).await;
            let source = cx.editor.downgrade();
            let (started, respond) = pending_locations(&cx, references, vec![target_location(&cx)]);
            let query = cx
                .update_editor(|editor, _, cx| {
                    if references {
                        let project = editor.project.clone().expect("project");
                        editor.find_all_references_locations(&project, cx)
                    } else {
                        editor.definition_locations_of_kind(GotoDefinitionKind::Symbol, cx)
                    }
                })
                .expect("location query");
            started.await.expect("query started");
            cx.editor = cx.update_workspace(|workspace, window, cx| {
                let replacement = cx.new(|cx| Editor::single_line(window, cx));
                workspace.active_pane().update(cx, |pane, cx| {
                    pane.remove_item(source.entity_id(), false, false, window, cx);
                    pane.add_item(Box::new(replacement.clone()), true, true, None, window, cx);
                });
                replacement
            });
            cx.run_until_parked();
            cx.background_executor
                .advance_clock(workspace::SERIALIZATION_THROTTLE_TIME);
            cx.run_until_parked();
            cx.update(|_, _| assert!(source.upgrade().is_none()));
            source.assert_released();
            respond.send(()).expect("release response");
            assert_eq!(query.await.expect("cancelled query").len(), 0);
        }
    }

    #[gpui::test]
    async fn test_pending_native_navigation_ignores_window_teardown(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for references in [false, true] {
            for split_or_multibuffer in [false, true] {
                for source_owned in [false, true] {
                    let mut editor_cx = navigation_context(cx).await;
                    let source = editor_cx.editor.downgrade();
                    let workspace = editor_cx.workspace.downgrade();
                    let server = editor_cx.lsp.server.clone();
                    let (started, respond) = pending_locations(
                        &editor_cx,
                        references,
                        vec![target_location(&editor_cx)],
                    );
                    let navigation =
                        native_navigation(&mut editor_cx, references, split_or_multibuffer);
                    let navigation = if source_owned {
                        editor_cx.update_editor(|editor, _, cx| {
                            editor.run_navigation_task(navigation, cx);
                        });
                        None
                    } else {
                        Some(navigation)
                    };
                    started.await.expect("query started");
                    editor_cx.deactivate_window();
                    editor_cx.update(|window, _| window.remove_window());
                    cx.update(|_| drop(editor_cx));
                    cx.run_until_parked();
                    cx.background_executor
                        .advance_clock(workspace::SERIALIZATION_THROTTLE_TIME);
                    cx.run_until_parked();
                    source.assert_released();
                    workspace.assert_released();
                    if let Some(navigation) = navigation {
                        respond.send(()).expect("release response");
                        assert_eq!(
                            navigation.await.expect("cancelled navigation"),
                            Navigated::No
                        );
                    } else {
                        assert_eq!(respond.send(()), Err(()));
                    }
                    drop(server);
                }
            }
        }
    }

    #[gpui::test]
    async fn test_pending_file_navigation_releases_workspace(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for absolute in [false, true] {
            let mut editor_cx = navigation_context(cx).await;
            let path = navigation_file_target(&mut editor_cx, absolute, false).await;
            editor_cx.deactivate_window();
            let source = editor_cx.editor.downgrade();
            let workspace = editor_cx.workspace.downgrade();
            let project = editor_cx.update_workspace(|workspace, _, _| workspace.project().clone());
            editor_cx.update_editor(|editor, window, cx| {
                editor.begin_navigation();
                let navigation = editor.navigate_to_hover_links(
                    None,
                    vec![file_link(path, (2, 3))],
                    None,
                    false,
                    window,
                    cx,
                );
                editor.run_navigation_task(navigation, cx);
            });
            while editor_cx.read(|cx| {
                project
                    .read(cx)
                    .buffer_store()
                    .read(cx)
                    .loading_buffers()
                    .next()
                    .is_none()
            }) {
                assert!(editor_cx.executor().tick(), "file load did not start");
            }
            editor_cx.update(|window, _| window.remove_window());
            cx.update(|_| drop(editor_cx));
            cx.read(|cx| {
                assert_eq!(
                    project
                        .read(cx)
                        .buffer_store()
                        .read(cx)
                        .loading_buffers()
                        .count(),
                    1
                );
            });
            workspace.assert_released();
            cx.run_until_parked();
            source.assert_released();
        }
    }

    #[gpui::test]
    async fn test_guarded_file_open_ignores_window_teardown(cx: &mut TestAppContext) {
        init_test(cx, |_| {});
        for absolute in [false, true] {
            for during_load in [false, true] {
                let mut cx = navigation_context(cx).await;
                let path = navigation_file_target(&mut cx, absolute, false).await;
                let project = cx.update_workspace(|workspace, _, _| workspace.project().clone());
                let open = cx.update_workspace(|workspace, window, cx| {
                    workspace.open_resolved_path_with_guard(path, window, cx, |_, _| true)
                });
                if during_load {
                    while cx.read(|cx| {
                        project
                            .read(cx)
                            .buffer_store()
                            .read(cx)
                            .loading_buffers()
                            .next()
                            .is_none()
                    }) {
                        assert!(cx.executor().tick(), "file load did not start");
                    }
                }
                cx.update(|window, _| window.remove_window());
                assert!(open.await.expect("cancelled file open").is_none());
            }
        }
    }

    async fn navigation_context(cx: &mut TestAppContext) -> EditorLspTestContext {
        let mut cx = EditorLspTestContext::new_rust(
            lsp::ServerCapabilities {
                definition_provider: Some(lsp::OneOf::Left(true)),
                references_provider: Some(lsp::OneOf::Left(true)),
                ..lsp::ServerCapabilities::default()
            },
            cx,
        )
        .await;
        cx.set_state(SOURCE);
        cx
    }

    async fn assert_pending_native_navigation_uses_source_pane(
        cx: &mut TestAppContext,
        split: bool,
        multibuffer: bool,
        remove_source: bool,
        references: bool,
        split_editor: bool,
        wrap_item: bool,
    ) {
        let mut cx = navigation_context(cx).await;
        let source_item_id = if split_editor {
            let buffer = cx.editor(|editor, _, cx| {
                editor
                    .buffer
                    .read(cx)
                    .as_singleton()
                    .expect("source buffer")
            });
            let (wrapper, source_item_id) = cx.update_workspace(|workspace, window, cx| {
                let multibuffer = cx.new(|_| MultiBuffer::new(Capability::ReadWrite));
                let project = workspace.project().clone();
                let workspace_handle = cx.entity();
                let wrapper = cx.new(|cx| {
                    SplittableEditor::new(
                        DiffViewStyle::Split,
                        multibuffer,
                        project,
                        workspace_handle,
                        window,
                        cx,
                    )
                });
                let source_item: Box<dyn ItemHandle> = if wrap_item {
                    Box::new(cx.new(|_| NavigationItem {
                        item: wrapper.clone(),
                    }))
                } else {
                    Box::new(wrapper.clone())
                };
                let source_item_id = source_item.item_id();
                workspace.active_pane().update(cx, |pane, cx| {
                    let source = pane.active_item().expect("source item").item_id();
                    pane.remove_item(source, false, false, window, cx);
                    pane.add_item(source_item, true, true, None, window, cx);
                });
                (wrapper, source_item_id)
            });
            cx.run_until_parked();
            cx.editor = cx.update(|_, cx| {
                wrapper.update(cx, |wrapper, cx| {
                    let snapshot = buffer.read(cx).text_snapshot();
                    let diff = cx.new(|cx| {
                        BufferDiff::new_with_base_text("fn target() {}\n", &snapshot, cx)
                    });
                    wrapper.update_excerpts_for_path(
                        PathKey::for_buffer(&buffer, cx),
                        buffer.clone(),
                        [Point::new(0, 0)..snapshot.max_point()],
                        0,
                        diff,
                        cx,
                    );
                    wrapper.rhs_editor().clone()
                })
            });
            let editors = cx.update(|_, cx| {
                let wrapper = wrapper.read(cx);
                [
                    wrapper.rhs_editor().clone(),
                    wrapper.lhs_editor().expect("left editor").clone(),
                ]
            });
            for editor in editors {
                let workspace = cx.workspace.clone();
                cx.read(|cx| {
                    assert_eq!(
                        Editor::containing_item(workspace.read(cx), editor.entity_id(), cx)
                            .map(|item| item.item_id()),
                        Some(source_item_id)
                    );
                });
            }
            cx.update_editor(|editor, window, cx| {
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.select_ranges([Point::new(1, 15)..Point::new(1, 15)]);
                });
            });
            source_item_id
        } else if wrap_item {
            let editor = cx.editor.clone();
            let source_item_id = cx.update_workspace(|workspace, window, cx| {
                let wrapper = cx.new(|_| NavigationItem {
                    item: editor.clone(),
                });
                workspace.active_pane().update(cx, |pane, cx| {
                    pane.remove_item(editor.entity_id(), false, false, window, cx);
                    pane.add_item(Box::new(wrapper.clone()), true, true, None, window, cx);
                });
                wrapper.entity_id()
            });
            cx.run_until_parked();
            source_item_id
        } else {
            cx.editor.entity_id()
        };
        let target_path = EditorLspTestContext::root_path().join("dir/target.rs");
        let fs = cx.update_workspace(|workspace, _, cx| workspace.project().read(cx).fs().clone());
        fs.as_fake()
            .insert_file(&target_path, b"fn target() {}\n".to_vec())
            .await;
        let mut locations = vec![lsp::Location {
            uri: lsp::Uri::from_file_path(&target_path).expect("target URI"),
            range: lsp::Range::new(lsp::Position::new(0, 3), lsp::Position::new(0, 9)),
        }];
        if multibuffer {
            locations.push(target_location(&cx));
        }
        let (source_pane, other_pane, other_editor) =
            cx.update_workspace(|workspace, window, cx| {
                let source_pane = workspace.active_pane().clone();
                let other_pane =
                    workspace.split_pane(source_pane.clone(), SplitDirection::Right, window, cx);
                let other_editor = cx.new(|cx| Editor::single_line(window, cx));
                other_pane.update(cx, |pane, cx| {
                    pane.add_item(Box::new(other_editor.clone()), true, true, None, window, cx);
                });
                (source_pane, other_pane, other_editor)
            });
        cx.update_editor(|editor, window, cx| window.focus(&editor.focus_handle(cx), cx));
        cx.run_until_parked();
        cx.update_workspace(|workspace, _, _| assert_eq!(workspace.active_pane(), &source_pane));
        let (request_started, respond) = pending_locations(&cx, references, locations);
        let navigation = native_navigation(
            &mut cx,
            references,
            if references { multibuffer } else { split },
        );
        request_started.await.expect("request started");
        let source_editor = cx.editor.clone();
        cx.update(|window, cx| {
            window.focus(&other_editor.focus_handle(cx), cx);
            if remove_source {
                source_pane.update(cx, |pane, cx| {
                    pane.remove_item(source_item_id, false, false, window, cx);
                });
            }
        });
        cx.run_until_parked();
        cx.update_workspace(|workspace, _, _| assert_eq!(workspace.active_pane(), &other_pane));
        respond.send(()).expect("release response");
        let navigated = navigation.await.expect("navigation result");
        cx.update_workspace(|workspace, _, cx| {
            assert_eq!(workspace.panes().len(), 2);
            if remove_source {
                assert_eq!(navigated, Navigated::No);
                assert_eq!(workspace.active_pane(), &other_pane);
                assert_eq!(workspace.active_item_as::<Editor>(cx), Some(other_editor));
                assert_eq!(source_pane.read(cx).items_len(), 0);
                assert_eq!(other_pane.read(cx).items_len(), 1);
            } else {
                assert_eq!(navigated, Navigated::Yes);
                let expected_pane = if split { &other_pane } else { &source_pane };
                assert_eq!(workspace.active_pane(), expected_pane);
                let target_editor = workspace
                    .active_item_as::<Editor>(cx)
                    .expect("target editor");
                assert_ne!(target_editor, source_editor);
                assert_ne!(target_editor, other_editor);
                let target_buffer = target_editor.read(cx).buffer().read(cx);
                assert_eq!(target_buffer.is_singleton(), !multibuffer);
                if let Some(buffer) = target_buffer.as_singleton() {
                    assert_eq!(buffer.read(cx).text(), "fn target() {}\n");
                    let selection = target_editor.update(cx, |editor, cx| {
                        let snapshot = editor.display_snapshot(cx);
                        editor.selections.newest::<Point>(&snapshot).range()
                    });
                    assert_eq!(
                        selection,
                        Point::new(0, 3)..Point::new(0, if references { 3 } else { 9 })
                    );
                }
                assert_eq!(
                    workspace
                        .pane_for_item_id(target_editor.entity_id())
                        .as_ref(),
                    Some(expected_pane)
                );
            }
        });
        if split_editor && !remove_source {
            let buffer = cx
                .update_workspace(|workspace, _, cx| {
                    workspace
                        .project()
                        .update(cx, |project, cx| project.open_local_buffer(target_path, cx))
                })
                .await
                .expect("replacement buffer");
            let (started, respond) = pending_locations(&cx, true, Vec::new());
            let navigation = native_navigation(&mut cx, true, false);
            cx.update_editor(|editor, _, cx| editor.run_navigation_task(navigation, cx));
            started.await.expect("old references started");
            cx.run_until_parked();
            let (started, respond_replacement) = pending_locations(&cx, true, Vec::new());
            cx.update_workspace(|workspace, _, cx| {
                let wrapper = Editor::containing_item(workspace, source_editor.entity_id(), cx)
                    .and_then(|item| item.act_as::<SplittableEditor>(cx))
                    .expect("source wrapper");
                wrapper.update(cx, |wrapper, cx| {
                    let snapshot = buffer.read(cx).text_snapshot();
                    let diff = cx.new(|cx| BufferDiff::new_with_base_text("", &snapshot, cx));
                    wrapper.update_excerpts_for_path(
                        PathKey::for_buffer(&buffer, cx),
                        buffer.clone(),
                        [Point::zero()..snapshot.max_point()],
                        0,
                        diff,
                        cx,
                    );
                });
            });
            cx.update_editor(|editor, window, cx| {
                let position = editor
                    .buffer
                    .read(cx)
                    .buffer_point_to_anchor(&buffer, Point::new(0, 3), cx)
                    .expect("replacement anchor");
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections.select_anchor_ranges([position..position]);
                });
                let navigation = editor
                    .find_all_references(&FindAllReferences::default(), window, cx)
                    .expect("replacement references");
                editor.run_navigation_task(navigation, cx);
            });
            started.await.expect("replacement references started");
            cx.run_until_parked();
            cx.editor(|editor, _, cx| {
                assert_eq!(editor.navigation.reference_sources.len(), 1);
                let (anchor, request) = editor
                    .navigation
                    .reference_sources
                    .first()
                    .expect("reservation");
                assert_eq!(anchor.buffer_id(), Some(buffer.read(cx).remote_id()));
                assert!(request.is_current(editor));
            });
            respond.send(()).ok();
            respond_replacement
                .send(())
                .expect("release replacement references");
            cx.run_until_parked();
            cx.editor(|editor, _, _| assert_eq!(editor.navigation.reference_sources.len(), 0));
        }
    }

    async fn navigation_file_target(
        cx: &mut EditorLspTestContext,
        absolute: bool,
        outside: bool,
    ) -> project::ResolvedPath {
        let path = if outside {
            EditorLspTestContext::root_path().with_file_name("outside.rs")
        } else {
            EditorLspTestContext::root_path().join("dir/target.rs")
        };
        let project = cx.update_workspace(|workspace, _, _| workspace.project().clone());
        let fs = cx.read(|cx| project.read(cx).fs().clone());
        fs.as_fake()
            .insert_file(&path, b"first\nsecond\n".to_vec())
            .await;
        if absolute {
            project::ResolvedPath::AbsPath {
                path: path.to_string_lossy().into_owned(),
                is_dir: false,
            }
        } else {
            project::ResolvedPath::ProjectPath {
                project_path: cx.read(|cx| {
                    project
                        .read(cx)
                        .find_project_path(&path, cx)
                        .expect("target project path")
                }),
                is_dir: false,
            }
        }
    }

    fn file_link(resolved_path: project::ResolvedPath, (row, column): (u32, u32)) -> HoverLink {
        HoverLink::File(hover_links::ResolvedFileTarget {
            resolved_path,
            row: Some(row),
            column: Some(column),
        })
    }

    fn local_target(editor: &Editor, cx: &App) -> Location {
        let buffer = editor.buffer.read(cx).as_singleton().expect("buffer");
        let snapshot = buffer.read(cx).snapshot();
        Location {
            buffer,
            range: snapshot.anchor_before(Point::new(0, 3))
                ..snapshot.anchor_after(Point::new(0, 9)),
        }
    }

    fn target_location(cx: &EditorLspTestContext) -> lsp::Location {
        lsp::Location {
            uri: cx.buffer_lsp_url.clone(),
            range: lsp::Range::new(lsp::Position::new(0, 3), lsp::Position::new(0, 9)),
        }
    }

    fn pending_response<R>(
        cx: &EditorLspTestContext,
        response: R::Result,
    ) -> (oneshot::Receiver<()>, oneshot::Sender<()>)
    where
        R: lsp::request::Request + 'static,
        R::Params: Send + 'static,
        R::Result: Send + 'static,
    {
        let (started_sender, started) = oneshot::channel();
        let (respond, released) = oneshot::channel();
        let mut pending = Some((started_sender, released, response));
        cx.set_request_handler::<R, _, _>(move |_, _, _| {
            let (started, released, response) = pending.take().expect("one navigation request");
            started.send(()).expect("request listener");
            async move {
                released.await.expect("release response");
                Ok(response)
            }
        });
        (started, respond)
    }

    fn pending_locations(
        cx: &EditorLspTestContext,
        references: bool,
        locations: Vec<lsp::Location>,
    ) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        if references {
            pending_response::<lsp::request::References>(cx, Some(locations))
        } else {
            pending_response::<lsp::request::GotoDefinition>(
                cx,
                Some(lsp::GotoDefinitionResponse::Array(locations)),
            )
        }
    }

    fn native_navigation(
        cx: &mut EditorLspTestContext,
        references: bool,
        split_or_multibuffer: bool,
    ) -> Task<Result<Navigated>> {
        cx.update_editor(|editor, window, cx| {
            let request = editor.navigation_request();
            let navigation = if references {
                editor
                    .find_all_references(
                        &FindAllReferences {
                            always_open_multibuffer: split_or_multibuffer,
                            open_results_in: Some(OpenResultsIn::MultiBuffer),
                        },
                        window,
                        cx,
                    )
                    .expect("reference query")
            } else {
                editor.go_to_definition_of_kind(
                    GotoDefinitionKind::Symbol,
                    split_or_multibuffer,
                    window,
                    cx,
                )
            };
            assert!(!request.is_current(editor));
            navigation
        })
    }

    async fn invalidate_response(
        cx: &mut EditorLspTestContext,
        (started, respond): (oneshot::Receiver<()>, oneshot::Sender<()>),
        disable: bool,
    ) {
        started.await.expect("request started");
        cx.update_editor(|editor, _, _| {
            if disable {
                editor.disable_lsp_data();
            } else {
                editor.begin_navigation();
            }
        });
        respond.send(()).expect("release response");
    }

    struct NavigationItem<T: Item> {
        item: Entity<T>,
    }

    impl<T: Item> Focusable for NavigationItem<T> {
        fn focus_handle(&self, cx: &App) -> FocusHandle {
            self.item.focus_handle(cx)
        }
    }

    impl<T: Item> EventEmitter<()> for NavigationItem<T> {}

    impl<T: Item> Render for NavigationItem<T> {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.item.clone()
        }
    }

    impl<T: Item> Item for NavigationItem<T> {
        type Event = ();

        fn tab_content_text(&self, detail: usize, cx: &App) -> SharedString {
            self.item.read(cx).tab_content_text(detail, cx)
        }

        fn act_as_type<'a>(
            &'a self,
            type_id: TypeId,
            handle: &'a Entity<Self>,
            cx: &'a App,
        ) -> Option<AnyEntity> {
            if type_id == TypeId::of::<Self>() {
                Some(AnyEntity::from(handle.clone()))
            } else {
                self.item.read(cx).act_as_type(type_id, &self.item, cx)
            }
        }

        fn added_to_workspace(
            &mut self,
            workspace: &mut Workspace,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.item.update(cx, |item, cx| {
                item.added_to_workspace(workspace, window, cx);
            });
        }

        fn set_nav_history(
            &mut self,
            history: ItemNavHistory,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.item.update(cx, |item, cx| {
                item.set_nav_history(history, window, cx);
            });
        }
    }

    fn assert_navigation_state(cx: &mut EditorLspTestContext, expected_state: &str) {
        cx.run_until_parked();
        cx.editor(|editor, _, _| assert_eq!(editor.navigation.reference_sources.len(), 0));
        cx.assert_editor_state(expected_state);
        let source_editor = cx.editor.clone();
        cx.update_workspace(|workspace, _, cx| {
            assert_eq!(workspace.panes().len(), 1);
            assert_eq!(workspace.active_pane().read(cx).items_len(), 1);
            assert_eq!(workspace.active_item_as::<Editor>(cx), Some(source_editor));
        });
    }
}
