#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point2D {
    pub x: f32,
    pub y: f32,
}

impl Point2D {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn from_array(arr: [f32; 2]) -> Self {
        Self { x: arr[0], y: arr[1] }
    }

    pub fn to_array(&self) -> [f32; 2] {
        [self.x, self.y]
    }

    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn distance_to(&self, other: Point2D) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn add(&self, other: Point2D) -> Point2D {
        Point2D {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

    pub fn sub(&self, other: Point2D) -> Point2D {
        Point2D {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect2D {
    pub min: Point2D,
    pub max: Point2D,
}

impl Rect2D {
    pub fn new(min: Point2D, max: Point2D) -> Self {
        Self { min, max }
    }

    pub fn from_points(start: Point2D, end: Point2D) -> Self {
        Self {
            min: Point2D::new(start.x.min(end.x), start.y.min(end.y)),
            max: Point2D::new(start.x.max(end.x), start.y.max(end.y)),
        }
    }

    pub fn from_point_size(x:f32,y:f32,width:f32,height:f32)-> Self {
        Self {
            min: Point2D::new(x, y),
            max: Point2D::new(x + width, y + height),
        }
    }

    pub fn center(&self) -> Point2D {
        Point2D::new((self.min.x + self.max.x) / 2.0, (self.min.y + self.max.y) / 2.0)
    }

    pub fn contain_point(&self, point: Point2D) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }
}
