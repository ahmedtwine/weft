macro_rules! hegel {
    ($ctx:expr, $name:ident($($arg:expr),*)) => {
        $crate::engine::checked($ctx, unsafe { hegel_c::$name($ctx, $($arg),*) })
    };
}

mod annotation;
mod engine;
mod generator;

pub use annotation::{Annotation, schema, types};
pub use engine::{Case, Ended, Engine, Halt, INTERESTING, INVALID, OVERRUN, VALID};
pub use generator::{Bound, Generator};
