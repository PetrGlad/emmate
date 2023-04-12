use iced::{Color, Element, Length, Point, Rectangle, Theme};
use iced::widget::{canvas, Canvas};
use iced::widget::canvas::{Cursor, Frame, Geometry, LineCap, Path, Stroke};

#[derive(Debug, /*Clone, Copy,*/ Default)]
pub struct Stave {
    pub radius: f32,
}

impl Stave {
    pub fn view(&self) -> Element<()> {
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

impl canvas::Program<()> for Stave {
    type State = ();

    fn draw(&self, _state: &Self::State, _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(bounds.size());
        let key_count = 88;
        let tone_step = bounds.height / key_count as f32;
        let time_step = bounds.width / 1000.0;

        // Grid
        let black_key = |tone: &i32| vec![1, 4, 6, 9, 11].contains(&(*tone % 12));
        for row in 0..key_count {
            let color = if black_key(&row) {
                Color::from_rgb(0.3, 0.3, 0.3)
            } else {
                Color::from_rgb(0.9, 0.9, 0.9)
            };
            let y = tone_step * row.to_owned() as f32;
            frame.stroke(&Path::line(Point { x: 0.0, y },
                                     Point { x: frame.width(), y }),
                         Stroke::default().with_color(color));
        }
        // Notes
        let mock_track = [(34, 28, 12), (45, 100, 30), (147, 30, 17)];
        for (t, duration, tone) in mock_track {
            let y = tone_step * tone as f32;
            let x = t as f32 * time_step;
            frame.stroke(&Path::line(Point { x, y },
                                     Point { x: x + (duration as f32 * time_step), y }),
                         Stroke::default()
                             .with_color(Color::from_rgb(0.4, 0.45, 0.55))
                             .with_width(tone_step * 0.9)
                             .with_line_cap(LineCap::Round));
        }

        // let background = Path::rectangle(Point::ORIGIN, frame.size());
        // frame.fill(&background, Color::WHITE);

        let circle = Path::circle(frame.center(), self.radius.into());
        frame.fill(&circle, Color::BLACK);
        vec![frame.into_geometry()]
    }
}
