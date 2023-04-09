use iced::{Color, Element, Length, Point, Rectangle, Theme};
use iced::event::Status;
use iced::mouse::Interaction;
use iced::widget::{canvas, Canvas};
use iced::widget::canvas::{Cursor, Event, Frame, Geometry, Path, Stroke};
use crate::Message;

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

    fn draw(&self, state: &Self::State, theme: &Theme, bounds: Rectangle, cursor: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(bounds.size());
        let tone_step = bounds.height / 100.0;
        let time_step = bounds.width / 1000.0;

        // let background = Path::rectangle(Point::ORIGIN, frame.size());
        // frame.fill(&background, Color::WHITE);

        frame.stroke(&Path::line(Point::ORIGIN, frame.center()), Stroke::default());
        let circle = Path::circle(frame.center(), self.radius.into());
        frame.fill(&circle, Color::BLACK);
        vec![frame.into_geometry()]
    }
}
