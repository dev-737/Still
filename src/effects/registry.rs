use super::{
    Assets, Effect, auto_crop::AutoCrop, blur::Blur, canvas::Canvas, contrast::Contrast,
    crop::Crop, draw_background::DrawBackground, draw_border::DrawBorder,
    draw_checkerboard::DrawCheckerboard, draw_image::DrawImage, draw_particles::DrawParticles,
    draw_text::DrawText, draw_text_ex::DrawTextEx, glow::Glow, pixelate::Pixelate, rotate::Rotate,
    rounded_corners::RoundedCorners, selective_color::SelectiveColor, shadow::Shadow,
    torn_edge::TornEdge,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt;

#[derive(Debug)]
pub enum BuildError {
    Unsupported(String),
    Invalid(String, String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(name) => write!(f, "{name} is not supported"),
            Self::Invalid(name, why) => write!(f, "{name}: {why}"),
        }
    }
}

impl std::error::Error for BuildError {}

type Builder = fn(Value, &mut dyn Assets) -> crate::capture::Result<Box<dyn Effect>>;

fn plain<E>(value: Value, _: &mut dyn Assets) -> crate::capture::Result<Box<dyn Effect>>
where
    E: Effect + DeserializeOwned + 'static,
{
    Ok(Box::new(serde_json::from_value::<E>(value)?))
}

const EFFECTS: &[(&str, Builder)] = &[
    ("AutoCrop", plain::<AutoCrop>),
    ("Blur", plain::<Blur>),
    ("Canvas", plain::<Canvas>),
    ("Contrast", plain::<Contrast>),
    ("Crop", plain::<Crop>),
    ("DrawBackground", plain::<DrawBackground>),
    ("DrawBorder", plain::<DrawBorder>),
    ("DrawCheckerboard", plain::<DrawCheckerboard>),
    ("DrawImage", DrawImage::build),
    ("DrawParticles", DrawParticles::build),
    ("DrawText", plain::<DrawText>),
    ("DrawTextEx", plain::<DrawTextEx>),
    ("Glow", plain::<Glow>),
    ("Pixelate", plain::<Pixelate>),
    ("Rotate", plain::<Rotate>),
    ("RoundedCorners", plain::<RoundedCorners>),
    ("SelectiveColor", plain::<SelectiveColor>),
    ("Shadow", plain::<Shadow>),
    ("TornEdge", plain::<TornEdge>),
];

pub fn build(
    kind: &str,
    settings: Value,
    assets: &mut dyn Assets,
) -> Result<Box<dyn Effect>, BuildError> {
    let (_, builder) = EFFECTS
        .iter()
        .find(|(name, _)| *name == kind)
        .ok_or_else(|| BuildError::Unsupported(kind.to_owned()))?;
    builder(settings, assets)
        .map_err(|error| BuildError::Invalid(kind.to_owned(), error.to_string()))
}

#[cfg(test)]
struct NoAssets;

#[cfg(test)]
impl Assets for NoAssets {
    fn image(&mut self, location: &str) -> crate::capture::Result<cairo::ImageSurface> {
        Err(format!("no image {location}").into())
    }
    fn folder(&mut self, location: &str) -> crate::capture::Result<Vec<cairo::ImageSurface>> {
        Err(format!("no folder {location}").into())
    }
}

#[cfg(test)]
pub fn build_all(json: &str) -> super::Pipeline {
    let effects: Vec<Value> = serde_json::from_str(json).unwrap();
    super::Pipeline::new(
        effects
            .into_iter()
            .map(|mut effect| {
                let kind = effect["$type"].as_str().unwrap().to_owned();
                build(&kind, effect.take(), &mut NoAssets).unwrap()
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_effects_are_reported_by_name() {
        let error = build("Sepia", Value::Null, &mut NoAssets).err().unwrap();
        assert_eq!(error.to_string(), "Sepia is not supported");
    }

    #[test]
    fn bad_settings_name_the_effect() {
        let error = build(
            "Canvas",
            serde_json::json!({"Margin": "wide"}),
            &mut NoAssets,
        )
        .err()
        .unwrap();
        assert!(error.to_string().starts_with("Canvas: "));
    }

    #[test]
    fn every_listed_effect_builds_from_empty_settings() {
        for (name, _) in EFFECTS {
            if matches!(*name, "DrawImage" | "DrawParticles") {
                continue;
            }
            assert!(
                build(name, serde_json::json!({}), &mut NoAssets).is_ok(),
                "{name}"
            );
        }
    }
}
