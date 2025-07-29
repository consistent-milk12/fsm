use crate::controller::actions::Action;
use crate::controller::event_processor::Event;
use crate::error::AppError;

pub trait EventHandler: Send + Sync {
    fn can_handle(&self, event: &Event) -> bool;
    fn handle(&mut self, event: Event) -> Result<Vec<Action>, AppError>;
    fn priority(&self) -> u8;
    fn name(&self) -> &'static str;
}

pub trait KeyEventHandler: Send + Sync {
    // Define methods specific to key event handling if needed
}
