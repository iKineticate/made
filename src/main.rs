mod config;
mod pinyin;
mod single_instance;
mod util;

use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result};
use arboard::Clipboard;
use crossterm::event::{Event, KeyCode};
use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    widgets::{
        Block, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, StatefulWidget, Widget,
    },
};
use win_hotkeys::HotkeyManager;
use win_hotkeys::VKey;

use crate::{config::CONFIG, pinyin::match_pinyin, single_instance::SingleInstance};

pub static CLIPBOARD: LazyLock<Mutex<Clipboard>> =
    LazyLock::new(|| Mutex::new(Clipboard::new().expect("Failed to create new clipboard")));

pub static UPDATE_TUI_TEXT: LazyLock<Arc<AtomicBool>> =
    LazyLock::new(|| Arc::new(AtomicBool::new(false)));

fn main() -> Result<()> {
    let _single_instance = SingleInstance::new()?;

    std::thread::spawn(move || {
        let mut hkm = HotkeyManager::new();

        hkm.register_hotkey(VKey::C, &[VKey::Menu], move || {
            let text = CLIPBOARD.lock().unwrap().get_text().unwrap();
            CONFIG.lock().unwrap().push_text(text);
            UPDATE_TUI_TEXT.store(true, Ordering::Relaxed);
        })
        .unwrap();

        hkm.event_loop();
    });

    ratatui::run(|terminal| Tui::default().run(terminal))?;

    Ok(())
}

struct TextList {
    items: Vec<String>,
    state: ListState,
}

pub struct Tui {
    exit: bool,
    //
    search_text: String,
    character_index: usize,
    //
    text_list: TextList,
    filtered_indices: Vec<usize>,
    //
    scrollbar_state: ScrollbarState,
}

impl Default for Tui {
    fn default() -> Self {
        let items = CONFIG.lock().unwrap().texts.clone();

        let mut tui = Self {
            exit: false,
            search_text: String::new(),
            character_index: 0,
            text_list: TextList {
                items,
                state: ListState::default().with_selected(Some(0)),
            },
            filtered_indices: Vec::new(),
            scrollbar_state: ScrollbarState::new(0),
        };

        tui.rebuild_filter();
        tui
    }
}

impl Tui {
    fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.exit {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            self.handle_events()?;
            self.update_text_list();
        }

        Ok(())
    }

    fn update_text_list(&mut self) {
        while UPDATE_TUI_TEXT.swap(false, Ordering::Relaxed) {
            self.text_list = TextList {
                items: CONFIG.lock().unwrap().texts.clone(),
                state: ListState::default().with_selected(Some(0)),
            };
            self.rebuild_filter();
        }
    }

    fn handle_events(&mut self) -> Result<()> {
        let _ = crossterm::event::poll(std::time::Duration::from_millis(250))
            .context("event poll failed")?;

        if let Event::Key(key) = crossterm::event::read().context("event read failed")? {
            if key.kind != crossterm::event::KeyEventKind::Press {
                return Ok(());
            }

            match key.code {
                KeyCode::Esc => {
                    if self.search_text.trim().is_empty() {
                        self.exit = true;
                    } else {
                        self.search_text.clear();
                        self.rebuild_filter();
                    }
                }
                KeyCode::Down => self.select_next(),
                KeyCode::Up => self.select_previous(),
                KeyCode::Left => {
                    let cursor_moved_left = self.character_index.saturating_sub(1);
                    self.character_index = self.clamp_cursor(cursor_moved_left);
                }
                KeyCode::Right => {
                    let cursor_moved_right = self.character_index.saturating_add(1);
                    self.character_index = self.clamp_cursor(cursor_moved_right);
                }
                KeyCode::Home => self.select_first(),
                KeyCode::End => self.select_last(),
                KeyCode::Backspace => {
                    self.delete_char();
                    self.rebuild_filter();
                }
                KeyCode::Char(to_insert) => {
                    self.enter_char(to_insert);
                    self.rebuild_filter();
                }
                KeyCode::Enter => {
                    if let Some(selected) = self.text_list.state.selected()
                        && let Some(&real_index) = self.filtered_indices.get(selected)
                    {
                        let text = self.text_list.items[real_index].clone();
                        CLIPBOARD.lock().unwrap().set_text(text)?;
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn byte_index(&self) -> usize {
        self.search_text
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.character_index)
            .unwrap_or(self.search_text.len())
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.search_text.chars().count())
    }

    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.search_text.insert(index, new_char);
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    fn select_next(&mut self) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }

        let next = match self.text_list.state.selected() {
            Some(i) if i + 1 < len => i + 1,
            _ => 0,
        };

        self.text_list.state.select(Some(next));
    }
    fn select_previous(&mut self) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }

        let prev = match self.text_list.state.selected() {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };

        self.text_list.state.select(Some(prev));
    }

    const fn select_first(&mut self) {
        self.text_list.state.select_first();
    }

    const fn select_last(&mut self) {
        self.text_list.state.select_last();
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            let before_char_to_delete = self.search_text.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.search_text.chars().skip(current_index);

            self.search_text = before_char_to_delete.chain(after_char_to_delete).collect();
            let cursor_moved_left = self.character_index.saturating_sub(1);
            self.character_index = self.clamp_cursor(cursor_moved_left);
        }
    }

    fn rebuild_filter(&mut self) {
        let search = self.search_text.trim();

        if search.is_empty() {
            self.filtered_indices.clear();
            self.text_list.state.select(None);
            return;
        }

        self.filtered_indices = self
            .text_list
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, text)| {
                let matched = match_pinyin(search, text);
                matched.then_some(i)
            })
            .collect();

        self.scrollbar_state = ScrollbarState::new(self.filtered_indices.len());
        // 修正选中状态
        if self.filtered_indices.is_empty() {
            self.text_list.state.select(None);
        } else {
            self.text_list.state.select(Some(0));
        }
    }
}

impl Widget for &mut Tui {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let main_layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(3),
        ]);
        let [header_area, content_area, search_area] = area.layout(&main_layout);

        Tui::render_header(header_area, buf);
        self.render_list(content_area, buf);
        self.render_scrollbar(content_area, buf);
        self.render_search(search_area, buf);
    }
}

impl Tui {
    fn render_header(area: Rect, buf: &mut Buffer) {
        Paragraph::new("made(玛德)")
            .bold()
            .centered()
            .render(area, buf);
    }

    fn render_search(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered().title(" 搜索");

        Paragraph::new(self.search_text.clone())
            .block(block)
            .centered()
            .render(area, buf);
    }

    fn render_scrollbar(&mut self, area: Rect, buf: &mut Buffer) {
        let content_length = self.filtered_indices.len();
        let position = self.text_list.state.selected().unwrap_or(0);

        self.scrollbar_state = self
            .scrollbar_state
            .content_length(content_length)
            .position(position);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        scrollbar.render(area, buf, &mut self.scrollbar_state);
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        let items: Vec<ListItem> = self
            .filtered_indices
            .iter()
            .enumerate()
            .map(|(display_index, &real_index)| {
                let text = &self.text_list.items[real_index];

                let background = if display_index % 2 == 0 {
                    Color::Rgb(25, 25, 25)
                } else {
                    Color::Rgb(42, 42, 42)
                };

                ListItem::new(text.clone()).bg(background)
            })
            .collect();

        let list = List::new(items)
            .block(Block::bordered().title(" 结果"))
            .highlight_style(
                Style::new()
                    .bg(Color::Rgb(66, 66, 66))
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">");

        StatefulWidget::render(list, area, buf, &mut self.text_list.state);
    }
}
