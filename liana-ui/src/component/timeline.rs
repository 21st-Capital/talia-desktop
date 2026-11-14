//! The installer's spending timeline, drawn as the Talia vault graph: one lane
//! per spending path, an amber key tile and a green key card on the left, and a
//! line that is dashed while the path waits and solid amber once it is open.

use iced::{
    advanced::text as advanced_text,
    alignment::Vertical,
    mouse,
    widget::canvas::{self, Canvas, Frame, LineDash, Path, Stroke},
    Color, Font, Pixels, Point, Rectangle, Renderer, Size,
};
use liana_i18n::t;

use crate::{font, theme::Theme, widget::Element};

/// A wallet shape the installer offers, with the policies it comes with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Custom,
    Inheritance,
    MultisigSecurity,
}

/// The spending policies of a wallet template over time.
pub fn spending_timeline<'a, M: 'a>(template: Template) -> Element<'a, M> {
    Canvas::<Template, M, Theme, Renderer>::new(template)
        .width(CANVAS_WIDTH)
        .height(CANVAS_HEIGHT)
        .into()
}

// The graph's colours are pinned: the same amber, card green and inks in every
// drawing of it, whatever palette the application runs with.
const AMBER: Color = Color::from_rgb8(0xF2, 0xA9, 0x00);
const CARD_GREEN: Color = Color::from_rgb8(0x12, 0x4A, 0x38);
const GROUND: Color = Color::from_rgb8(0x0F, 0x15, 0x13);
const WHITE: Color = Color::WHITE;
/// A lane that is still waiting: its dashed line and its starting dot.
const WAITING: Color = Color::from_rgb8(0x5C, 0x60, 0x5F);
const AXIS_INK: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.72);
const FOOTNOTE_INK: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.50);
const GRIDLINE: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.16);
const GROUND_BORDER: Color = Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.08);

const CANVAS_WIDTH: f32 = 800.0;
const CANVAS_HEIGHT: f32 = 320.0;
const PAD: f32 = 28.0;
const GROUND_RADIUS: f32 = 16.0;
const HAIRLINE: f32 = 1.0;

/// Axis labels hang from here, centred on their gridline.
const AXIS_BOTTOM: f32 = 60.0;
const AXIS_WIDTH_MAX: f32 = 170.0;
/// An axis label keeps this much of the step between gridlines to itself.
const AXIS_STEP_INSET: f32 = 16.0;
/// Gridlines start under the axis text, never level with it.
const GRID_TOP: f32 = 68.0;
const GRID_BOTTOM: f32 = 258.0;
const GRID_WIDTH: f32 = 2.0;
const LANES_TOP: f32 = 76.0;
const LANES_BOTTOM: f32 = 252.0;
const LANE_GAP_MAX: f32 = 36.0;
const FOOTNOTE_TOP: f32 = 270.0;

const TILE_WIDTH: f32 = 36.0;
const TILE_GAP: f32 = 5.0;
const BOX_RADIUS: f32 = 6.0;
const CARD_WIDTH_MIN: f32 = 180.0;
const CARD_WIDTH_MAX: f32 = 270.0;
const CARD_INSET_LEFT: f32 = 11.0;
const CARD_INSET_RIGHT: f32 = 12.0;
/// The rhythm both columns are laid out from, so that a key's glyph always
/// sits on that key's name.
const CARD_PAD_V: f32 = 8.0;
const TEXT_LINE: f32 = 16.0;
const KEY_SPACE: f32 = 3.0;

/// Key card to the first gridline.
const LINE_GAP: f32 = 34.0;
/// Where the waiting path opens, as a fraction of the timeline.
const OPENING_RATIO: f32 = 0.52;
const LINE_WIDTH: f32 = 2.0;
const DASH: [f32; 2] = [6.0, 5.0];
const DOT_RADIUS: f32 = 5.0;
/// Ground-coloured ring that lifts a dot off its line.
const DOT_RING: f32 = 7.5;
const ARROW_LENGTH: f32 = 10.0;
const ARROW_HALF: f32 = 6.0;
const PILL_PAD_X: f32 = 10.0;
const PILL_PAD_Y: f32 = 5.0;
const PILL_RADIUS: f32 = 5.0;
/// A pill keeps this far from a dot or an arrowhead.
const PILL_CLEAR: f32 = 14.0;

/// Bootstrap Icons, the font liana-ui registers for [`crate::icon`].
const ICONS: Font = Font::with_name("bootstrap-icons");
const KEY_GLYPH: char = '\u{F44E}';
const KEY_GLYPH_SIZE: f32 = 16.0;

const CARD_SIZES: &[f32] = &[13.0, 12.0, 11.0, 10.0];
const AXIS_SIZES: &[f32] = &[13.0, 12.0, 11.0];
const PILL_SIZES: &[f32] = &[10.0, 9.0];
const FOOTNOTE_SIZES: &[f32] = &[13.0, 12.0, 11.0];
const LINE_HEIGHT: f32 = 1.0;

/// Steps of the bisection that balances a wrapped text over its lines.
const BISECT_STEPS: usize = 8;
/// Slack absorbing the rounding of a laid-out line height.
const ROUNDING: f32 = 0.5;

const EVENTS: [Event; 2] = [Event::Receipt, Event::Timelock];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Receipt,
    Timelock,
}

impl Event {
    fn label(self) -> String {
        match self {
            Event::Receipt => t!("installer-template-event-receipt"),
            Event::Timelock => t!("installer-template-event-timelock"),
        }
    }
}

/// One spending path: the keys it names, how many of them must sign, and the
/// event that opens it.
struct Policy {
    name: String,
    threshold: usize,
    keys: Vec<String>,
    opens: Event,
}

impl Policy {
    /// The quorum goes in the title when there is one to state.
    fn title(&self) -> String {
        if self.keys.len() > 1 {
            format!("{} {}/{}:", self.name, self.threshold, self.keys.len())
        } else {
            format!("{}:", self.name)
        }
    }
}

impl Template {
    /// The path that always spends, then the one the timelock opens.
    fn policies(self) -> [Policy; 2] {
        let (threshold, primary, recovery) = match self {
            Template::Custom => (
                1,
                vec![t!("installer-template-custom-primary")],
                vec![t!("installer-template-custom-recovery")],
            ),
            Template::Inheritance => (
                1,
                vec![t!("installer-primary-key")],
                vec![t!("installer-inheritance-key")],
            ),
            Template::MultisigSecurity => (
                2,
                vec![
                    t!("installer-primary-key-number", number = 1),
                    t!("installer-primary-key-number", number = 2),
                ],
                vec![
                    t!("installer-primary-key-number", number = 1),
                    t!("installer-primary-key-number", number = 2),
                    t!("installer-recovery-key"),
                ],
            ),
        };
        [
            Policy {
                name: t!("installer-template-policy-primary"),
                threshold,
                keys: primary,
                opens: Event::Receipt,
            },
            Policy {
                name: t!("installer-template-policy-recovery"),
                threshold,
                keys: recovery,
                opens: Event::Timelock,
            },
        ]
    }
}

/// A policy laid out: its texts at the size they fit, and the height its card
/// takes.
struct Lane {
    title: Fitted,
    names: Vec<Fitted>,
    opens: Event,
    height: f32,
}

/// A text laid out at the largest size that fits, with the bounds it takes.
struct Fitted {
    text: canvas::Text,
    bounds: Size,
}

impl<M> canvas::Program<M, Theme, Renderer> for Template {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        Painter { frame: &mut frame }.draw(*self);
        vec![frame.into_geometry()]
    }
}

struct Painter<'a> {
    frame: &'a mut Frame,
}

impl Painter<'_> {
    fn draw(&mut self, template: Template) {
        let lanes = lanes(template);
        let card_width = lanes
            .iter()
            .flat_map(|lane| std::iter::once(&lane.title).chain(&lane.names))
            .map(|text| text.bounds.width + CARD_INSET_LEFT + CARD_INSET_RIGHT)
            .fold(CARD_WIDTH_MIN, f32::max)
            .min(CARD_WIDTH_MAX);
        let x_start = PAD + TILE_WIDTH + TILE_GAP + card_width + LINE_GAP;
        let x_end = CANVAS_WIDTH - PAD;
        let x_opening = x_start + ((x_end - x_start) * OPENING_RATIO).round();
        let x_of = |event: Event| match event {
            Event::Receipt => x_start,
            Event::Timelock => x_opening,
        };

        self.ground();
        let axis_width = (x_opening - x_start - AXIS_STEP_INSET).min(AXIS_WIDTH_MAX);
        for event in EVENTS {
            self.gridline(x_of(event));
            self.axis_label(x_of(event), event, axis_width);
        }

        // The lanes share the room evenly, stacked around its middle.
        let taken: f32 = lanes.iter().map(|lane| lane.height).sum();
        let room = LANES_BOTTOM - LANES_TOP;
        let gap = ((room - taken) / (lanes.len() as f32 - 1.0)).clamp(0.0, LANE_GAP_MAX);
        let stack = taken + gap * (lanes.len() as f32 - 1.0);
        let mut top = (LANES_TOP + (room - stack) / 2.0).round();
        for lane in &lanes {
            let y = (top + lane.height / 2.0).round();
            self.key_tile(top, lane);
            self.key_card(top, card_width, lane);
            self.timeline(x_start, x_of(lane.opens), x_end, y);
            top += lane.height + gap;
        }
        self.footnote();
    }

    /// The panel the graph sits on. It keeps its own dark ground so the green
    /// cards and the amber read the same whatever surrounds it.
    fn ground(&mut self) {
        let ground = Path::rounded_rectangle(
            Point::new(HAIRLINE / 2.0, HAIRLINE / 2.0),
            Size::new(CANVAS_WIDTH - HAIRLINE, self.frame.height() - HAIRLINE),
            GROUND_RADIUS.into(),
        );
        self.frame.fill(&ground, GROUND);
        self.frame.stroke(
            &ground,
            Stroke::default()
                .with_color(GROUND_BORDER)
                .with_width(HAIRLINE),
        );
    }

    /// One flat colour from top to bottom: nothing in the graph fades.
    fn gridline(&mut self, x: f32) {
        self.frame.fill_rectangle(
            Point::new(x - GRID_WIDTH / 2.0, GRID_TOP),
            Size::new(GRID_WIDTH, GRID_BOTTOM - GRID_TOP),
            GRIDLINE,
        );
    }

    fn axis_label(&mut self, x: f32, event: Event, width: f32) {
        let header = fit(
            label(event.label(), font::MEDIUM, AXIS_INK),
            AXIS_SIZES,
            width,
            3,
        );
        self.text(
            &header.text,
            Point::new(x, AXIS_BOTTOM),
            advanced_text::Alignment::Center,
            Vertical::Bottom,
        );
    }

    /// The amber tile: one white key per key of the lane, each level with its
    /// name. A lane with a single key centres it.
    fn key_tile(&mut self, top: f32, lane: &Lane) {
        let tile = Path::rounded_rectangle(
            Point::new(PAD, top),
            Size::new(TILE_WIDTH, lane.height),
            BOX_RADIUS.into(),
        );
        self.frame.fill(&tile, AMBER);

        let key = glyph(KEY_GLYPH, KEY_GLYPH_SIZE, WHITE);
        let x = PAD + TILE_WIDTH / 2.0;
        for i in 0..lane.names.len() {
            let y = if lane.names.len() == 1 {
                top + lane.height / 2.0
            } else {
                name_center(top, i)
            };
            self.text(
                &key,
                Point::new(x, y),
                advanced_text::Alignment::Center,
                Vertical::Center,
            );
        }
    }

    /// The green card: the lane's title in bold, then each key's name on its
    /// own line. Hard edges all round.
    fn key_card(&mut self, top: f32, width: f32, lane: &Lane) {
        let left = PAD + TILE_WIDTH + TILE_GAP;
        let card = Path::rounded_rectangle(
            Point::new(left, top),
            Size::new(width, lane.height),
            BOX_RADIUS.into(),
        );
        self.frame.fill(&card, CARD_GREEN);

        let x = left + CARD_INSET_LEFT;
        self.text(
            &lane.title.text,
            Point::new(x, top + CARD_PAD_V + TEXT_LINE / 2.0),
            advanced_text::Alignment::Left,
            Vertical::Center,
        );
        for (i, name) in lane.names.iter().enumerate() {
            self.text(
                &name.text,
                Point::new(x, name_center(top, i)),
                advanced_text::Alignment::Left,
                Vertical::Center,
            );
        }
    }

    /// Dashed grey while the path waits, then solid amber from the opening dot
    /// to the arrowhead, with the ACTIVE pill where it opens.
    fn timeline(&mut self, x_start: f32, x_open: f32, x_end: f32, y: f32) {
        if x_open > x_start {
            self.frame.stroke(
                &Path::line(Point::new(x_start, y), Point::new(x_open, y)),
                Stroke {
                    line_dash: LineDash {
                        segments: &DASH,
                        offset: 0,
                    },
                    ..Stroke::default().with_color(WAITING).with_width(LINE_WIDTH)
                },
            );
            self.dot(x_start, y, WAITING);
        }

        let x_arrow = x_end - ARROW_LENGTH;
        self.frame.fill_rectangle(
            Point::new(x_open, y - LINE_WIDTH / 2.0),
            Size::new(x_arrow - x_open + 1.0, LINE_WIDTH),
            AMBER,
        );
        let arrow = Path::new(|path| {
            path.move_to(Point::new(x_end, y));
            path.line_to(Point::new(x_arrow, y - ARROW_HALF));
            path.line_to(Point::new(x_arrow, y + ARROW_HALF));
            path.close();
        });
        self.frame.fill(&arrow, AMBER);
        self.dot(x_open, y, AMBER);

        // A path open from the start wears its pill before the next gridline;
        // one that opens later wears it between its dot and its arrowhead.
        let x_until = if x_open > x_start {
            x_arrow
        } else {
            x_start + (x_end - x_start) * OPENING_RATIO
        };
        self.active_pill(x_open + PILL_CLEAR, x_until - PILL_CLEAR, y);
    }

    fn dot(&mut self, x: f32, y: f32, color: Color) {
        let center = Point::new(x, y);
        self.frame.fill(&Path::circle(center, DOT_RING), GROUND);
        self.frame.fill(&Path::circle(center, DOT_RADIUS), color);
    }

    /// White on amber, centred on the line between `x1` and `x2`.
    fn active_pill(&mut self, x1: f32, x2: f32, y: f32) {
        let state = fit(
            label(t!("pill-active").to_uppercase(), font::BOLD, WHITE),
            PILL_SIZES,
            x2 - x1 - 2.0 * PILL_PAD_X,
            1,
        );
        let size = Size::new(
            (2.0 * PILL_PAD_X + state.bounds.width).round(),
            (2.0 * PILL_PAD_Y + state.bounds.height).round(),
        );
        let left = ((x1 + x2 - size.width) / 2.0).round();
        let pill = Path::rounded_rectangle(
            Point::new(left, y - size.height / 2.0),
            size,
            PILL_RADIUS.into(),
        );
        self.frame.fill(&pill, AMBER);
        self.text(
            &state.text,
            Point::new(left + size.width / 2.0, y),
            advanced_text::Alignment::Center,
            Vertical::Center,
        );
    }

    fn footnote(&mut self) {
        let note = fit(
            label(
                t!("installer-template-footnote"),
                font::REGULAR,
                FOOTNOTE_INK,
            ),
            FOOTNOTE_SIZES,
            CANVAS_WIDTH - 2.0 * PAD,
            2,
        );
        self.text(
            &note.text,
            Point::new(PAD, FOOTNOTE_TOP),
            advanced_text::Alignment::Left,
            Vertical::Top,
        );
    }

    fn text(
        &mut self,
        text: &canvas::Text,
        position: Point,
        align_x: advanced_text::Alignment,
        align_y: Vertical,
    ) {
        self.frame.fill_text(canvas::Text {
            position,
            align_x,
            align_y,
            ..text.clone()
        });
    }
}

/// Each policy with its texts fitted to the widest card the graph allows.
fn lanes(template: Template) -> Vec<Lane> {
    let text_max = CARD_WIDTH_MAX - CARD_INSET_LEFT - CARD_INSET_RIGHT;
    template
        .policies()
        .into_iter()
        .map(|policy| {
            let title = fit(
                label(policy.title(), font::BOLD, WHITE),
                CARD_SIZES,
                text_max,
                1,
            );
            let last = policy.keys.len() - 1;
            let names: Vec<Fitted> = policy
                .keys
                .iter()
                .enumerate()
                .map(|(i, key)| {
                    // Keys that sign together are joined by a plus.
                    let name = if i < last {
                        format!("{key} +")
                    } else {
                        key.clone()
                    };
                    fit(label(name, font::REGULAR, WHITE), CARD_SIZES, text_max, 1)
                })
                .collect();
            let height =
                2.0 * CARD_PAD_V + TEXT_LINE + (KEY_SPACE + TEXT_LINE) * names.len() as f32;
            Lane {
                title,
                names,
                opens: policy.opens,
                height,
            }
        })
        .collect()
}

/// Middle of the line the `index`th key's name sits on, in a lane starting at
/// `top`. The tile and the card both read it, so glyph and name stay level.
fn name_center(top: f32, index: usize) -> f32 {
    top + CARD_PAD_V
        + TEXT_LINE
        + (KEY_SPACE + TEXT_LINE) * index as f32
        + KEY_SPACE
        + TEXT_LINE / 2.0
}

fn label(content: String, font: Font, color: Color) -> canvas::Text {
    canvas::Text {
        content,
        color,
        font,
        line_height: advanced_text::LineHeight::Relative(LINE_HEIGHT),
        shaping: advanced_text::Shaping::Advanced,
        ..canvas::Text::default()
    }
}

fn glyph(code: char, size: f32, color: Color) -> canvas::Text {
    let mut glyph = label(code.to_string(), ICONS, color);
    glyph.size = Pixels(size);
    glyph
}

/// Largest of `sizes` that fits on one line, else on up to `max_lines`
/// balanced lines, else the smallest size in the box it is given.
fn fit(mut text: canvas::Text, sizes: &[f32], max_width: f32, max_lines: usize) -> Fitted {
    for size in sizes {
        text.size = Pixels(*size);
        text.max_width = f32::INFINITY;
        let single = measure(&text);
        if single.width <= max_width {
            return Fitted {
                text,
                bounds: single,
            };
        }
        if max_lines > 1 {
            text.max_width = max_width;
            let wrapped = measure(&text);
            if wrapped.height <= size * LINE_HEIGHT * max_lines as f32 + ROUNDING {
                text.max_width = balanced_width(&text, wrapped.height);
                let bounds = measure(&text);
                return Fitted { text, bounds };
            }
        }
    }
    text.max_width = max_width;
    let bounds = measure(&text);
    Fitted { text, bounds }
}

/// Narrowest box that keeps the wrapped line count, so the lines come out
/// balanced rather than one long line and a stub.
fn balanced_width(text: &canvas::Text, wrapped_height: f32) -> f32 {
    let mut probe = text.clone();
    let (mut low, mut high) = (widest_word(text), text.max_width);
    if low >= high {
        return high;
    }
    for _ in 0..BISECT_STEPS {
        let mid = (low + high) / 2.0;
        probe.max_width = mid;
        if measure(&probe).height <= wrapped_height {
            high = mid;
        } else {
            low = mid;
        }
    }
    high
}

/// No box narrower than this can hold the text without breaking a word.
fn widest_word(text: &canvas::Text) -> f32 {
    let mut probe = text.clone();
    probe.max_width = f32::INFINITY;
    text.content
        .split_whitespace()
        .map(|word| {
            probe.content = word.to_owned();
            measure(&probe).width
        })
        .fold(0.0, f32::max)
}

/// The bounds `Frame::fill_text` will lay the text out in.
fn measure(text: &canvas::Text) -> Size {
    use advanced_text::Paragraph as _;

    type Layout = <Renderer as advanced_text::Renderer>::Paragraph;
    Layout::with_text(advanced_text::Text {
        content: text.content.as_str(),
        bounds: Size::new(text.max_width, f32::INFINITY),
        size: text.size,
        line_height: text.line_height,
        font: text.font,
        align_x: text.align_x,
        align_y: text.align_y,
        shaping: text.shaping,
        wrapping: advanced_text::Wrapping::default(),
    })
    .min_bounds()
}
