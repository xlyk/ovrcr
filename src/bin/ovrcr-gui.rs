use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke};
use ovrcr::gui::{Demo, Terminal, input::encode_event};
use std::fs;
use std::sync::Arc;
use std::time::Duration;

#[cfg(target_os = "macos")]
#[path = "ovrcr-gui/native_glyph.rs"]
mod native_glyph;

const TERMINAL_FONT_SIZE: f32 = 11.0;
const JETBRAINS_REGULAR: &str = "ovrcr-jetbrains-regular";
const JETBRAINS_BOLD: &str = "ovrcr-jetbrains-bold";
const JETBRAINS_ITALIC: &str = "ovrcr-jetbrains-italic";
const JETBRAINS_BOLD_ITALIC: &str = "ovrcr-jetbrains-bold-italic";

#[derive(Clone)]
struct TerminalFonts {
    regular: FontId,
    bold: FontId,
    italic: FontId,
    bold_italic: FontId,
    native_bold: bool,
    native_italic: bool,
    native_bold_italic: bool,
    #[cfg(target_os = "macos")]
    fallback: std::cell::RefCell<native_glyph::NativeGlyphs>,
}

impl TerminalFonts {
    fn current_monospace() -> Self {
        let regular = FontId::monospace(TERMINAL_FONT_SIZE);
        Self {
            regular: regular.clone(),
            bold: regular.clone(),
            italic: regular.clone(),
            bold_italic: regular,
            native_bold: false,
            native_italic: false,
            native_bold_italic: false,
            #[cfg(target_os = "macos")]
            fallback: Default::default(),
        }
    }

    fn for_cell(&self, bold: bool, italic: bool) -> (&FontId, bool) {
        match (bold, italic) {
            (true, true) => (&self.bold_italic, !self.native_bold_italic),
            (true, false) => (&self.bold, false),
            (false, true) => (&self.italic, !self.native_italic),
            (false, false) => (&self.regular, false),
        }
    }
}

fn named_font(name: &str) -> FontId {
    FontId {
        size: TERMINAL_FONT_SIZE,
        family: FontFamily::Name(name.into()),
    }
}

fn install_font(definitions: &mut FontDefinitions, name: &str, path: &std::path::Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    definitions
        .font_data
        .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
    let mut family = vec![name.into()];
    family.extend(definitions.families[&FontFamily::Monospace].iter().cloned());
    definitions
        .families
        .insert(FontFamily::Name(name.into()), family);
    true
}

fn install_nerd_font(
    definitions: &mut FontDefinitions,
    name: &str,
    font_dir: &std::path::Path,
    style: &str,
) -> bool {
    install_font(
        definitions,
        name,
        &font_dir.join(format!("JetBrainsMonoNerdFont-{style}.ttf")),
    ) || install_font(
        definitions,
        name,
        &font_dir.join(format!("JetBrainsMonoNerdFontMono-{style}.ttf")),
    )
}

fn install_terminal_fonts(context: &egui::Context) -> TerminalFonts {
    let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned()) else {
        return TerminalFonts::current_monospace();
    };
    let font_dir = home.join("Library/Fonts");
    let mut definitions = FontDefinitions::default();
    if install_font(
        &mut definitions,
        "ovrcr-symbols",
        std::path::Path::new("/System/Library/Fonts/Apple Symbols.ttf"),
    ) {
        definitions
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .push("ovrcr-symbols".into());
    }
    if !install_nerd_font(&mut definitions, JETBRAINS_REGULAR, &font_dir, "Regular") {
        context.set_fonts(definitions);
        return TerminalFonts::current_monospace();
    }
    let native_bold = install_nerd_font(&mut definitions, JETBRAINS_BOLD, &font_dir, "Bold");
    let native_italic = install_nerd_font(&mut definitions, JETBRAINS_ITALIC, &font_dir, "Italic");
    let native_bold_italic = install_nerd_font(
        &mut definitions,
        JETBRAINS_BOLD_ITALIC,
        &font_dir,
        "BoldItalic",
    );
    context.set_fonts(definitions);
    let regular = named_font(JETBRAINS_REGULAR);
    TerminalFonts {
        regular: regular.clone(),
        bold: if native_bold {
            named_font(JETBRAINS_BOLD)
        } else {
            regular.clone()
        },
        italic: if native_italic {
            named_font(JETBRAINS_ITALIC)
        } else {
            regular.clone()
        },
        bold_italic: if native_bold_italic {
            named_font(JETBRAINS_BOLD_ITALIC)
        } else if native_bold {
            named_font(JETBRAINS_BOLD)
        } else {
            regular.clone()
        },
        native_bold,
        native_italic,
        native_bold_italic,
        #[cfg(target_os = "macos")]
        fallback: Default::default(),
    }
}

#[test]
#[cfg(target_os = "macos")]
fn terminal_fonts_render_mockup_glyphs_or_default_symbols() {
    let context = egui::Context::default();
    let terminal_fonts = install_terminal_fonts(&context);
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        ui.fonts_mut(|fonts| {
            let nerd_font = matches!(
                &terminal_fonts.regular.family,
                FontFamily::Name(name) if name.as_ref() == JETBRAINS_REGULAR
            );
            let symbols = if nerd_font {
                "▼▶󰉋⠋├└"
            } else {
                "⠋"
            };
            for font in [
                &terminal_fonts.regular,
                &terminal_fonts.bold,
                &terminal_fonts.italic,
                &terminal_fonts.bold_italic,
            ] {
                assert!(
                    fonts.has_glyphs(font, symbols),
                    "missing status symbols in {font:?}"
                );
            }
        });
    });
    output.textures_delta.clear();
}

#[test]
#[cfg(target_os = "macos")]
fn terminal_paints_missing_unicode_as_native_images() {
    assert_native_unicode_paint(true);
    assert_native_unicode_paint(false);
}

#[cfg(all(test, target_os = "macos"))]
fn assert_native_unicode_paint(installed_fonts: bool) {
    let context = egui::Context::default();
    let fonts = if installed_fonts {
        install_terminal_fonts(&context)
    } else {
        TerminalFonts::current_monospace()
    };
    let mut parser = vt100::Parser::new(1, 12, 0);
    parser.process("\x1b[31mRED 界🙂 END\x1b[0m".as_bytes());
    assert_eq!(parser.screen().contents(), "RED 界🙂 END");
    assert!(parser.screen().cell(0, 4).unwrap().is_wide());
    assert!(parser.screen().cell(0, 6).unwrap().is_wide());
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        paint_terminal(
            ui,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(120.0, 20.0)),
            egui::vec2(10.0, 20.0),
            &fonts,
            parser.screen(),
        );
    });
    let images: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) => Some((shape.clip_rect, mesh)),
            _ => None,
        })
        .collect();
    let textures = output.textures_delta.set.clone();
    output.textures_delta.clear();
    assert_eq!(
        images.len(),
        2,
        "missing glyphs need native artwork, not replacement galleys"
    );
    for ((clip, mesh), column) in images.iter().zip([4.0, 6.0]) {
        let bounds = mesh.calc_bounds();
        assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
        assert!(bounds.left() >= column * 10.0 - 1.0);
        assert!(bounds.right() <= (column + 2.0) * 10.0);
        assert!(clip.right() <= 120.0);
        let deltas = textures.get(&mesh.texture_id).unwrap();
        let egui::ImageData::Color(image) = &deltas[0].image;
        assert!(
            image.pixels.iter().any(|pixel| pixel.a() > 0),
            "native glyph must have visible pixels"
        );
        if column == 4.0 {
            assert!(
                image
                    .pixels
                    .iter()
                    .any(|p| p.r() > 0 && p.g() == 0 && p.b() == 0),
                "CJK fallback must retain the terminal foreground color"
            );
        } else {
            assert!(
                image.pixels.iter().any(|p| p.r() > p.g() && p.g() > p.b()),
                "emoji must contain its native color artwork, not a monochrome replacement box"
            );
        }
    }
    output.textures_delta.clear();
}

#[test]
fn terminal_paints_reserved_icon_cell_before_foreground_glyph() {
    let context = egui::Context::default();
    let fonts = TerminalFonts::current_monospace();
    let mut parser = vt100::Parser::new(1, 2, 0);
    parser.process("󰉋".as_bytes());
    parser.process(b"\x1b[?25l");
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        paint_terminal(
            ui,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(20.0, 10.0)),
            egui::vec2(10.0, 10.0),
            &fonts,
            parser.screen(),
        );
    });
    let text = output
        .shapes
        .iter()
        .position(|shape| matches!(shape.shape, egui::Shape::Text(_) | egui::Shape::Mesh(_)))
        .expect("icon must produce a foreground glyph");
    let backgrounds = output
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(index, shape)| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(30, 30, 46) => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();
    output.textures_delta.clear();
    assert_eq!(
        backgrounds.len(),
        3,
        "terminal and both icon cells are filled"
    );
    assert!(
        backgrounds.iter().all(|index| *index < text),
        "the icon's reserved empty cell must be filled before its glyph"
    );
}

struct App {
    terminal: Option<Terminal>,
    demo: Demo,
    fonts: TerminalFonts,
    error: Option<String>,
    closing: bool,
}

impl App {
    fn close(&mut self) -> anyhow::Result<()> {
        if let Some(mut terminal) = self.terminal.take() {
            terminal.stop()?;
        }
        self.demo.shutdown()
    }
}

impl App {
    fn show(&mut self, ui: &mut egui::Ui) {
        let open_palette =
            ui.input_mut(|input| input.consume_key(egui::Modifiers::MAC_CMD, egui::Key::K));
        if ui.input(|input| input.viewport().close_requested()) && !self.closing {
            match self.close() {
                Ok(()) => self.closing = true,
                Err(error) => {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.error = Some(format!(
                        "Cleanup failed: {error:#}. Demo retained at {}",
                        self.demo.root().display()
                    ));
                }
            }
        }
        egui::Frame::central_panel(ui.style())
            .inner_margin(egui::Margin::ZERO)
            .show(ui, |ui| {
                let status = match self.terminal.as_mut().map(Terminal::status).transpose() {
                    Ok(status) => status.flatten(),
                    Err(error) => {
                        self.error = Some(format!("{error:#}"));
                        None
                    }
                };
                if let Some(status) = &status {
                    ui.horizontal(|ui| {
                        ui.label(format!("Dashboard exited: {status}"));
                        if ui.button("Restart dashboard").clicked() || open_palette {
                            self.terminal.take();
                            match self.demo.dashboard(40, 120, ui.ctx().clone()) {
                                Ok(terminal) => {
                                    self.terminal = Some(terminal);
                                    self.error = None;
                                }
                                Err(error) => self.error = Some(format!("{error:#}")),
                            }
                        }
                    });
                }
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                let Some(terminal) = self.terminal.as_mut() else {
                    return;
                };
                let cell = ui.fonts_mut(|fonts| {
                    egui::vec2(
                        fonts.glyph_width(&self.fonts.regular, 'M'),
                        fonts.row_height(&self.fonts.regular),
                    )
                });
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
                if open_palette
                    || response.clicked()
                    || ui.memory(|memory| memory.focused().is_none())
                {
                    response.request_focus();
                }
                if open_palette && let Err(error) = terminal.send(b"\x07:") {
                    self.error = Some(format!("{error:#}"));
                }
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: true,
                        },
                    )
                });
                let rows = (rect.height() / cell.y).floor().max(1.0) as u16;
                let cols = (rect.width() / cell.x).floor().max(1.0) as u16;
                if let Err(error) = terminal.resize(rows, cols) {
                    self.error = Some(format!("{error:#}"));
                }
                let screen = terminal.screen();
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "OVRCR terminal")
                });
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    node.set_role(egui::accesskit::Role::Terminal);
                    node.set_label("OVRCR terminal");
                    node.set_value(screen.contents());
                });
                paint_terminal(ui, rect, cell, &self.fonts, &screen);
                // Individual rows avoid accessibility clients truncating one large text value.
                for (row, text) in screen.rows(0, cols).enumerate() {
                    if !text.trim().is_empty() {
                        let row_rect = egui::Rect::from_min_size(
                            rect.min + egui::vec2(0.0, row as f32 * cell.y),
                            egui::vec2(rect.width(), cell.y),
                        );
                        let row_response =
                            ui.interact(row_rect, response.id.with(row), egui::Sense::hover());
                        row_response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text)
                        });
                    }
                }
                if status.is_none() {
                    for event in ui.input(|input| input.events.clone()) {
                        if !response.has_focus()
                            && !matches!(event, egui::Event::PointerButton { .. })
                        {
                            continue;
                        }
                        let bytes = encode_event(&event, &screen, rect, cell);
                        if !bytes.is_empty()
                            && let Err(error) = terminal.send(&bytes)
                        {
                            self.error = Some(format!("{error:#}"));
                        }
                    }
                }
            });
        // Observe quiet child exits too; terminal output requests immediate repaints.
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(error) = self.close() {
            eprintln!(
                "OVRCR GUI cleanup: {error:#}; retained {}",
                self.demo.root().display()
            );
        }
    }
}

#[test]
fn command_palette_opens_when_another_control_has_focus() -> anyhow::Result<()> {
    let context = egui::Context::default();
    let executable = std::env::current_exe()?
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ovrcr");
    let demo = Demo::start(&executable)?;
    let root = demo.root().to_owned();
    let terminal = demo.dashboard(40, 120, context.clone())?;
    let mut app = App {
        terminal: Some(terminal),
        demo,
        fonts: TerminalFonts::current_monospace(),
        error: None,
        closing: false,
    };
    for detached in [false, true] {
        if detached {
            app.terminal.as_mut().unwrap().stop()?;
        }
        let mut other_text = String::new();
        let mut output = context.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::K,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        mac_cmd: true,
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |ui| {
                let other = ui.text_edit_singleline(&mut other_text);
                other.request_focus();
                app.show(ui);
                assert_ne!(
                    ui.memory(|m| m.focused()),
                    Some(other.id),
                    "Cmd-K must move focus into the popup"
                );
                assert!(
                    !ui.input(|i| i.events.iter().any(|e| matches!(
                        e,
                        egui::Event::Key {
                            key: egui::Key::K,
                            pressed: true,
                            ..
                        }
                    ))),
                    "shortcut must be consumed globally"
                );
            },
        );
        output.textures_delta.clear();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !app
            .terminal
            .as_ref()
            .unwrap()
            .screen()
            .contents()
            .contains("Command palette")
        {
            anyhow::ensure!(std::time::Instant::now() < deadline, "palette never opened");
            std::thread::yield_now();
        }
    }
    app.close()?;
    assert!(!root.exists());
    Ok(())
}

fn paint_terminal(
    ui: &egui::Ui,
    rect: egui::Rect,
    cell_size: egui::Vec2,
    fonts: &TerminalFonts,
    screen: &vt100::Screen,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let background = Color32::from_rgb(30, 30, 46);
    painter.rect_filled(rect, 0.0, background);
    let (rows, cols) = screen.size();
    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let origin =
                rect.min + egui::vec2(f32::from(col) * cell_size.x, f32::from(row) * cell_size.y);
            let mut fg = color(cell.fgcolor(), Color32::from_rgb(205, 214, 244));
            let mut bg = color(cell.bgcolor(), background);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            painter.rect_filled(egui::Rect::from_min_size(origin, cell_size), 0.0, bg);
        }
    }
    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let origin =
                rect.min + egui::vec2(f32::from(col) * cell_size.x, f32::from(row) * cell_size.y);
            let width = if cell.is_wide() { 2.0 } else { 1.0 } * cell_size.x;
            let cell_rect = egui::Rect::from_min_size(origin, egui::vec2(width, cell_size.y));
            let mut fg = color(cell.fgcolor(), Color32::from_rgb(205, 214, 244));
            let mut bg = color(cell.bgcolor(), background);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.dim() {
                fg = fg.gamma_multiply(0.65);
            }
            let contents = cell.contents();
            if !contents.is_empty() {
                let (font, synthetic_italic) = fonts.for_cell(cell.bold(), cell.italic());
                #[cfg(target_os = "macos")]
                // egui 0.36's has_glyphs rejects every glyph in the face that
                // also supplies the replacement character. Query real coverage.
                let native = !ui.fonts_mut(|view| {
                    let mut family = view.fonts.font(&font.family);
                    let characters = family.characters();
                    contents.chars().all(|ch| characters.contains_key(&ch))
                }) && fonts.fallback.borrow_mut().paint(
                    ui,
                    cell_rect.intersect(rect),
                    font,
                    cell,
                    fg,
                );
                #[cfg(not(target_os = "macos"))]
                let native = false;
                if !native {
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        contents,
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color: fg,
                            italics: synthetic_italic,
                            ..Default::default()
                        },
                    );
                    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                    painter.galley(origin, galley.clone(), fg);
                    if cell.bold() && !fonts.native_bold {
                        painter.galley(origin + egui::vec2(0.5, 0.0), galley, fg);
                    }
                }
            }
            if cell.underline() {
                painter.line_segment(
                    [
                        cell_rect.left_bottom() - egui::vec2(0.0, 1.0),
                        cell_rect.right_bottom() - egui::vec2(0.0, 1.0),
                    ],
                    Stroke::new(1.0, fg),
                );
            }
        }
    }
    if !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        let origin =
            rect.min + egui::vec2(f32::from(col) * cell_size.x, f32::from(row) * cell_size.y);
        painter.rect_stroke(
            egui::Rect::from_min_size(origin, cell_size),
            0.0,
            Stroke::new(1.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
}

fn color(color: vt100::Color, default: Color32) -> Color32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(index) => {
            const ANSI: [[u8; 3]; 16] = [
                [0, 0, 0],
                [205, 0, 0],
                [0, 205, 0],
                [205, 205, 0],
                [0, 0, 238],
                [205, 0, 205],
                [0, 205, 205],
                [229, 229, 229],
                [127, 127, 127],
                [255, 0, 0],
                [0, 255, 0],
                [255, 255, 0],
                [92, 92, 255],
                [255, 0, 255],
                [0, 255, 255],
                [255, 255, 255],
            ];
            let [r, g, b] = if index < 16 {
                ANSI[usize::from(index)]
            } else if index < 232 {
                let index = index - 16;
                let value = |v| if v == 0 { 0 } else { 55 + 40 * v };
                [value(index / 36), value(index / 6 % 6), value(index % 6)]
            } else {
                [8 + 10 * (index - 232); 3]
            };
            Color32::from_rgb(r, g, b)
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OVRCR GUI")
            .with_inner_size([1100.0, 900.0])
            .with_min_inner_size([480.0, 320.0]),
        ..Default::default()
    };
    eframe::run_native(
        "OVRCR GUI",
        options,
        Box::new(|creation| {
            let fonts = install_terminal_fonts(&creation.egui_ctx);
            let executable = std::env::current_exe()?.with_file_name("ovrcr");
            let demo = Demo::start(&executable)?;
            let terminal = demo.dashboard(40, 120, creation.egui_ctx.clone())?;
            Ok(Box::new(App {
                terminal: Some(terminal),
                demo,
                fonts,
                error: None,
                closing: false,
            }))
        }),
    )
}
