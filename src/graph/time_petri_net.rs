use super::petri_net::Shape;

// Token's color implement for the state of Thread
#[derive(Debug, Clone)]
enum Color {
    Red,
    Yellow,
    Green,
    Grey,
    Bule,
}

impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Color::Red => write!(f, "red"),
            _ => {
                write!(f, "red")
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Token {
    size: usize,
    color: Color,
}

#[derive(Debug, Clone)]
struct TimePlace {
    name: String,
    shape: Shape,
    token: Token,
}
