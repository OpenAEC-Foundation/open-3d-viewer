//! Units and number formatting for measurements. Values are kept in SI units (m, m², m³, kg,
//! radians) and shown in the unit and with the number of decimals the user picked. Dutch uses a
//! decimal comma and a thousands dot, English the other way round.

use openaec_ui::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Length,
    Area,
    Volume,
    Weight,
    Angle,
    Count,
}

impl Category {
    pub const ALL: [Category; 6] =
        [Category::Length, Category::Area, Category::Volume, Category::Weight, Category::Angle, Category::Count];

    pub fn label(self, l: Language) -> &'static str {
        match self {
            Category::Length => l.text("Lengte", "Length"),
            Category::Area => l.text("Oppervlakte", "Area"),
            Category::Volume => l.text("Volume", "Volume"),
            Category::Weight => l.text("Gewicht", "Weight"),
            Category::Angle => l.text("Hoek", "Angle"),
            Category::Count => l.text("Tellen", "Count"),
        }
    }

    /// Whether values of this category can be added up.
    pub fn summable(self) -> bool {
        !matches!(self, Category::Angle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Mm,
    Cm,
    M,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaUnit {
    Mm2,
    Cm2,
    M2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeUnit {
    Cm3,
    Dm3,
    M3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightUnit {
    Kg,
    T,
}

impl LengthUnit {
    pub const ALL: [LengthUnit; 3] = [LengthUnit::Mm, LengthUnit::Cm, LengthUnit::M];
    pub fn label(self) -> &'static str {
        match self {
            LengthUnit::Mm => "mm",
            LengthUnit::Cm => "cm",
            LengthUnit::M => "m",
        }
    }
    fn factor(self) -> f64 {
        match self {
            LengthUnit::Mm => 1000.0,
            LengthUnit::Cm => 100.0,
            LengthUnit::M => 1.0,
        }
    }
}

impl AreaUnit {
    pub const ALL: [AreaUnit; 3] = [AreaUnit::Mm2, AreaUnit::Cm2, AreaUnit::M2];
    pub fn label(self) -> &'static str {
        match self {
            AreaUnit::Mm2 => "mm²",
            AreaUnit::Cm2 => "cm²",
            AreaUnit::M2 => "m²",
        }
    }
    fn factor(self) -> f64 {
        match self {
            AreaUnit::Mm2 => 1e6,
            AreaUnit::Cm2 => 1e4,
            AreaUnit::M2 => 1.0,
        }
    }
}

impl VolumeUnit {
    pub const ALL: [VolumeUnit; 3] = [VolumeUnit::Cm3, VolumeUnit::Dm3, VolumeUnit::M3];
    pub fn label(self) -> &'static str {
        match self {
            VolumeUnit::Cm3 => "cm³",
            VolumeUnit::Dm3 => "dm³",
            VolumeUnit::M3 => "m³",
        }
    }
    fn factor(self) -> f64 {
        match self {
            VolumeUnit::Cm3 => 1e6,
            VolumeUnit::Dm3 => 1e3,
            VolumeUnit::M3 => 1.0,
        }
    }
}

impl WeightUnit {
    pub const ALL: [WeightUnit; 2] = [WeightUnit::Kg, WeightUnit::T];
    pub fn label(self) -> &'static str {
        match self {
            WeightUnit::Kg => "kg",
            WeightUnit::T => "t",
        }
    }
    fn factor(self) -> f64 {
        match self {
            WeightUnit::Kg => 1.0,
            WeightUnit::T => 0.001,
        }
    }
}

/// Display unit and decimals per category.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitSettings {
    pub length: LengthUnit,
    pub length_decimals: u8,
    pub area: AreaUnit,
    pub area_decimals: u8,
    pub volume: VolumeUnit,
    pub volume_decimals: u8,
    pub weight: WeightUnit,
    pub weight_decimals: u8,
    pub angle_decimals: u8,
}

impl Default for UnitSettings {
    fn default() -> Self {
        Self {
            length: LengthUnit::Mm,
            length_decimals: 0,
            area: AreaUnit::M2,
            area_decimals: 2,
            volume: VolumeUnit::M3,
            volume_decimals: 3,
            weight: WeightUnit::Kg,
            weight_decimals: 1,
            angle_decimals: 1,
        }
    }
}

pub const MAX_DECIMALS: u8 = 4;

impl UnitSettings {
    /// An SI value in the display unit: (value, unit label, decimals).
    pub fn display(&self, category: Category, value: f64) -> (f64, &'static str, u8) {
        match category {
            Category::Length => (value * self.length.factor(), self.length.label(), self.length_decimals),
            Category::Area => (value * self.area.factor(), self.area.label(), self.area_decimals),
            Category::Volume => (value * self.volume.factor(), self.volume.label(), self.volume_decimals),
            Category::Weight => (value * self.weight.factor(), self.weight.label(), self.weight_decimals),
            Category::Angle => (value.to_degrees(), "°", self.angle_decimals),
            Category::Count => (value, "", 0),
        }
    }

    /// "1.234 mm", "12,50 m²", "90,0°" (Dutch) or "1,234 mm", ... (English).
    pub fn format(&self, l: Language, category: Category, value: f64) -> String {
        let (v, unit, decimals) = self.display(category, value);
        let number = format_number(l, v, decimals);
        match category {
            Category::Angle => format!("{number}°"),
            Category::Count => number,
            _ => format!("{number} {unit}"),
        }
    }

    /// A coordinate in the length unit, without the unit.
    pub fn coordinate(&self, l: Language, meters: f64) -> String {
        format_number(l, meters * self.length.factor(), self.length_decimals)
    }
}

/// A number with a fixed number of decimals and thousands separators.
pub fn format_number(l: Language, value: f64, decimals: u8) -> String {
    let (decimal, thousands) = if l == Language::Dutch { (',', '.') } else { ('.', ',') };
    let mut s = format!("{:.*}", decimals as usize, value.abs());
    // never "-0,0"
    let negative = value < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0');
    let frac = s.find('.').map(|i| s.split_off(i)[1..].to_string());
    let mut grouped = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            grouped.push(thousands);
        }
        grouped.push(c);
    }
    if let Some(frac) = frac {
        grouped.push(decimal);
        grouped.push_str(&frac);
    }
    if negative {
        grouped.insert(0, '-');
    }
    grouped
}

pub fn format_density(l: Language, kg_per_m3: f64) -> String {
    format!("{} kg/m³", format_number(l, kg_per_m3, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dutch_and_english_separators() {
        assert_eq!(format_number(Language::Dutch, 1234.5, 1), "1.234,5");
        assert_eq!(format_number(Language::English, 1234567.891, 2), "1,234,567.89");
        assert_eq!(format_number(Language::Dutch, -0.0004, 2), "0,00");
        assert_eq!(format_number(Language::Dutch, -12.5, 0), "-13");
        assert_eq!(format_number(Language::Dutch, 999.0, 0), "999");
    }

    #[test]
    fn units() {
        let u = UnitSettings::default();
        assert_eq!(u.format(Language::Dutch, Category::Length, 1.0), "1.000 mm");
        assert_eq!(u.format(Language::Dutch, Category::Area, 12.345), "12,35 m²");
        assert_eq!(u.format(Language::Dutch, Category::Angle, std::f64::consts::FRAC_PI_2), "90,0°");
    }
}
