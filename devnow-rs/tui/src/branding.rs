use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

pub(crate) const ACCENT: Color = Color::Rgb(135, 206, 250);
pub(crate) const HIGHLIGHT: Color = Color::Rgb(185, 230, 255);
pub(crate) const SHADOW: Color = Color::Rgb(67, 137, 176);
pub(crate) const TEXT: Color = Color::Rgb(224, 244, 255);
pub(crate) const ROBOT_MIN_WIDTH: usize = 28;

pub(crate) fn footer_style() -> Style {
    Style::default().fg(TEXT).bg(Color::Black)
}

const ROBOT: [&str; 18] = [
    ".......BBBBBB.......",
    ".....BBBBHHBBBB.....",
    "....BBBBBHHBBBBB....",
    "...BBBBBBHHBBBBBB...",
    "...BBBBBBBBBBBBBB...",
    "..SSBBBBBBBBBBBBSS..",
    "..SSBBFFFFFFFFBBSS..",
    "..SSBFF..FF..FFBSS..",
    "..SSBFF..FF..FFBSS..",
    "...BBFFFFFFFFFFBB...",
    "....BBFFFFFFFFBB....",
    ".....BBFF..FFBB.....",
    "......BBFFFFBB......",
    ".......BBBBBB.......",
    "......SSFFFFSS......",
    "....BBBBBBBBBBBB....",
    "...BBBHHBBBBHHBBB...",
    "..BBBHHBBBBBBHHBBB..",
];
const WORDMARK: [&str; 5] = [
    "####  ##### #   #  ###  #   #  ###  #   #",
    "#   # #     #   # #   # ##  # #   # #   #",
    "#   # ####  #   # #   # # # # #   # # # #",
    "#   # #      # #  #   # #  ## #   # ## ##",
    "####  #####   #    ###  #   #  ###  #   #",
];

pub(crate) fn brand_lines(width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = if width >= ROBOT_MIN_WIDTH {
        ROBOT
            .chunks_exact(2)
            .enumerate()
            .map(|(index, rows)| {
                let mut spans = rows[0]
                    .bytes()
                    .zip(rows[1].bytes())
                    .map(|(top, bottom)| {
                        Span::from("\u{2580}")
                            .fg(pixel_color(top))
                            .bg(pixel_color(bottom))
                    })
                    .collect::<Vec<_>>();
                let lettering = if width >= ROBOT[0].len() + 1 + WORDMARK[0].len() {
                    match index {
                        2..=6 => WORDMARK[index - 2],
                        _ => "",
                    }
                } else if index == 4 {
                    "DEVONOW"
                } else {
                    ""
                };
                if !lettering.is_empty() {
                    spans.push(" ".into());
                    spans.push(lettering.replace('#', "\u{2588}").fg(ACCENT));
                }
                Line::from(spans).fg(ACCENT).bg(Color::Black)
            })
            .collect::<Vec<_>>()
    } else {
        vec![
            Line::from("DEVONOW".chars().take(width).collect::<String>())
                .fg(ACCENT)
                .bg(Color::Black),
        ]
    };
    if width >= ROBOT_MIN_WIDTH {
        lines.push(Line::default().fg(ACCENT).bg(Color::Black));
    }
    lines.push(
        Line::from("\u{2500}".repeat(width))
            .fg(ACCENT)
            .bg(Color::Black),
    );
    lines
}

fn pixel_color(pixel: u8) -> Color {
    match pixel {
        b'B' => ACCENT,
        b'H' => HIGHLIGHT,
        b'S' => SHADOW,
        b'F' => TEXT,
        b'.' => Color::Black,
        _ => unreachable!("invalid branding pixel: {pixel}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::style::Style;

    #[test]
    fn branding_fits_every_terminal_width() {
        for width in 0..=120 {
            let lines = brand_lines(width);
            assert!(lines.iter().all(|line| line.width() <= width));
            for line in lines {
                assert_eq!(line.style, Style::default().fg(ACCENT).bg(Color::Black));
            }
        }
    }

    #[test]
    fn classic_hero_preserves_square_pixels_and_face_colors() {
        assert!(ROBOT.iter().all(|row| row.len() == 20));
        let lines = brand_lines(56);
        assert_eq!(lines.len(), 11);
        assert_eq!(
            lines[3].spans[7],
            Span::from("\u{2580}").fg(TEXT).bg(Color::Black)
        );
        assert_eq!(
            lines[3].spans[5],
            Span::from("\u{2580}").fg(ACCENT).bg(TEXT)
        );
    }

    #[test]
    fn classic_hero_pixel_colors_snapshot() {
        let lines = brand_lines(56);
        let pixels = lines[..9]
            .iter()
            .flat_map(|line| {
                (0..2).map(move |half| {
                    line.spans[..20]
                        .iter()
                        .map(|span| {
                            let color = if half == 0 {
                                span.style.fg
                            } else {
                                span.style.bg
                            };
                            match color {
                                Some(ACCENT) => 'B',
                                Some(HIGHLIGHT) => 'H',
                                Some(SHADOW) => 'S',
                                Some(TEXT) => 'F',
                                Some(Color::Black) => '.',
                                other => panic!("unexpected hero color: {other:?}"),
                            }
                        })
                        .collect::<String>()
                })
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(pixels, @r###"
        .......BBBBBB.......
        .....BBBBHHBBBB.....
        ....BBBBBHHBBBBB....
        ...BBBBBBHHBBBBBB...
        ...BBBBBBBBBBBBBB...
        ..SSBBBBBBBBBBBBSS..
        ..SSBBFFFFFFFFBBSS..
        ..SSBFF..FF..FFBSS..
        ..SSBFF..FF..FFBSS..
        ...BBFFFFFFFFFFBB...
        ....BBFFFFFFFFBB....
        .....BBFF..FFBB.....
        ......BBFFFFBB......
        .......BBBBBB.......
        ......SSFFFFSS......
        ....BBBBBBBBBBBB....
        ...BBBHHBBBBHHBBB...
        ..BBBHHBBBBBBHHBBB..
        "###);
    }
}
