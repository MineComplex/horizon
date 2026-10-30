pub mod camel;
pub mod camel_ai;
pub mod camel_husk;
pub use self::camel::{Camel, CamelTrait, CamelCamelBodyRotationControl, CamelCamelLookControl, CamelCamelMoveControl};
pub use self::camel_ai::{CamelAi, CamelAiCamelPanic, CamelAiRandomSitting};
pub use self::camel_husk::{CamelHusk};
