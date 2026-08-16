use crossterm::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextElement {
    Identifier,
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Normal,
    Definition,
    Boolean,
    Special,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiElement {
    Background,
    SearchMatch,
    CurrentMatch,
    ModeNormal,
    ModeInsert,
    ModeVisual,
    StatusBar,
    StatusBarInactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeElement {
    Text(TextElement),
    Ui(UiElement),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: Color,
    pub bg: Option<Color>,
}

impl Style {
    pub const fn new(fg: Color) -> Self {
        Self { fg, bg: None }
    }

    pub const fn with_bg(fg: Color, bg: Color) -> Self {
        Self { fg, bg: Some(bg) }
    }
}

pub struct Theme;

impl Theme {
    pub fn color_for(element: ThemeElement) -> Style {
        use ThemeElement::*;
        match element {
            Text(text) => color_for_text(text),
            Ui(ui) => color_for_ui(ui),
        }
    }
}

fn color_for_text(element: TextElement) -> Style {
    use TextElement::*;
    match element {
        Identifier => Style::new(Color::White),
        String => Style::new(Color::Blue),
        Keyword => Style::new(Color::Rgb { r: 255, g: 90, b: 88 }),
        Comment => Style::new(Color::Rgb {
            r: 135,
            g: 141,
            b: 145,
        }),
        Number => Style::new(Color::Rgb {
            r: 176,
            g: 161,
            b: 255,
        }),
        Type => Style::new(Color::Rgb {
            r: 84,
            g: 179,
            b: 132,
        }),
        Normal => Style::new(Color::White),
        Definition => Style::new(Color::Rgb {
            r: 158,
            g: 208,
            b: 114,
        }),
        Boolean => Style::new(Color::Rgb { r: 255, g: 90, b: 88 }),
        Special => Style::new(Color::Magenta),
    }
}

fn color_for_ui(element: UiElement) -> Style {
    use UiElement::*;
    match element {
        Background => Style::new(Color::Reset),
        ModeNormal => Style::new(Color::Reset),
        ModeInsert => Style::new(Color::Reset),
        ModeVisual => Style::new(Color::Reset),
        SearchMatch => Style::with_bg(Color::Black, Color::Yellow),
        CurrentMatch => Style::with_bg(Color::Black, Color::Cyan),
        StatusBar => Style::new(Color::Reset),
        StatusBarInactive => Style::new(Color::Reset),
    }
}
