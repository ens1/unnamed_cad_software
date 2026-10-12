//! Source-derived engineering and finish columns shared by BOMs and cut lists.
//! Missing requirements remain blank; optical presets never manufacture a spec.
use crate::{
    Document, Id, Material, Part,
    appearance::Finish,
    materials::{Family, Properties, PropertyBasis, PropertyKey},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "field", rename_all = "snake_case")]
pub enum Column {
    MaterialFamily,
    MaterialGrade,
    MaterialCondition,
    Density,
    EngineeringProperty { property: PropertyKey },
    FinishCategory,
    FinishDescription,
    FinishProcess,
    FinishSpecification,
    FinishDesignation,
    CoatingThickness,
    SurfaceRoughness,
    FinishNotes,
    FaceFinishes,
}
impl Column {
    pub fn choices() -> Vec<Self> {
        let mut choices = vec![
            Self::MaterialFamily,
            Self::MaterialGrade,
            Self::MaterialCondition,
            Self::Density,
            Self::FinishCategory,
            Self::FinishDescription,
            Self::FinishProcess,
            Self::FinishSpecification,
            Self::FinishDesignation,
            Self::CoatingThickness,
            Self::SurfaceRoughness,
            Self::FinishNotes,
            Self::FaceFinishes,
        ];
        choices.extend(
            PropertyKey::all()
                .iter()
                .map(|property| Self::EngineeringProperty {
                    property: *property,
                }),
        );
        choices
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::MaterialFamily => "Material family",
            Self::MaterialGrade => "Material grade",
            Self::MaterialCondition => "Material condition",
            Self::Density => "Density (kg/m³)",
            Self::FinishCategory => "Finish category",
            Self::FinishDescription => "Finish description",
            Self::FinishProcess => "Finish process",
            Self::FinishSpecification => "Finish specification",
            Self::FinishDesignation => "Finish designation",
            Self::CoatingThickness => "Coating thickness (µm)",
            Self::SurfaceRoughness => "Finish Ra (µm)",
            Self::FinishNotes => "Finish requirements",
            Self::FaceFinishes => "Authored face finishes",
            Self::EngineeringProperty { property } => match property {
                PropertyKey::YoungsModulusPa => "Young's modulus (GPa)",
                PropertyKey::PoissonRatio => "Poisson's ratio",
                PropertyKey::YieldStrengthPa => "Yield strength (MPa)",
                PropertyKey::TensileStrengthPa => "Tensile strength (MPa)",
                PropertyKey::ShearModulusPa => "Shear modulus (GPa)",
                PropertyKey::ThermalConductivityWPerMK => "Conductivity (W/mK)",
                PropertyKey::SpecificHeatJPerKgK => "Specific heat (J/kgK)",
                PropertyKey::ThermalExpansionPerK => "Expansion (µm/mK)",
                PropertyKey::ElectricalResistivityOhmM => "Resistivity (µohm m)",
                PropertyKey::ElongationRatio => "Elongation (%)",
                PropertyKey::MeltingTemperatureK => "Melting point (K)",
            },
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Data {
    pub material_family: Option<Family>,
    pub material_grade: String,
    pub material_condition: String,
    pub density_kg_m3: Option<f64>,
    pub engineering_properties: Properties,
    pub finish_category: String,
    pub finish_description: String,
    pub finish_manufacturing: Option<crate::appearance::ManufacturingFinish>,
    #[serde(default, skip_serializing_if = "crate::sketch_actions::is_false")]
    pub mixed_finish_requirements: bool,
    pub face_finishes: Vec<String>,
}
impl Data {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn from_definitions(
        document: &Document,
        material: Option<&Material>,
        finish: Option<&Finish>,
        part: Option<&Part>,
    ) -> Self {
        let metadata = material.and_then(|m| m.metadata.as_ref());
        let finish_metadata = finish.and_then(|f| f.metadata.as_ref());
        Self {
            material_family: metadata.map(|m| m.family),
            material_grade: metadata.map(|m| m.grade.clone()).unwrap_or_default(),
            material_condition: metadata.map(|m| m.condition.clone()).unwrap_or_default(),
            density_kg_m3: material.map(|m| m.density_kg_m3),
            engineering_properties: material.map(|m| m.properties.clone()).unwrap_or_default(),
            finish_category: finish_metadata
                .map(|m| m.category.clone())
                .unwrap_or_default(),
            finish_description: finish_metadata
                .map(|m| m.description.clone())
                .unwrap_or_default(),
            finish_manufacturing: finish.and_then(|f| f.manufacturing.clone()),
            mixed_finish_requirements: false,
            face_finishes: part
                .into_iter()
                .flat_map(|p| &p.face_finishes)
                .map(|f| {
                    document
                        .finishes
                        .get(&f.finish)
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| f.finish.clone())
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        }
    }
    /// Definition grouping may intentionally merge different occurrence finishes.
    pub fn merge_finish(&mut self, other: &Self) {
        if self.finish_category != other.finish_category {
            self.finish_category = "Multiple finish categories".into();
        }
        if self.finish_description != other.finish_description {
            self.finish_description = "Multiple finish descriptions".into();
        }
        if self.finish_manufacturing != other.finish_manufacturing {
            self.finish_manufacturing = None;
            self.mixed_finish_requirements = true;
        }
        self.mixed_finish_requirements |= other.mixed_finish_requirements;
        self.face_finishes.extend(other.face_finishes.clone());
        self.face_finishes.sort();
        self.face_finishes.dedup();
    }
    pub fn cell(&self, column: &Column) -> String {
        if self.mixed_finish_requirements
            && matches!(
                column,
                Column::FinishProcess
                    | Column::FinishSpecification
                    | Column::FinishDesignation
                    | Column::CoatingThickness
                    | Column::SurfaceRoughness
                    | Column::FinishNotes
            )
        {
            return "Multiple requirements".into();
        }
        let requirements = self.finish_manufacturing.as_ref();
        match column {
            Column::MaterialFamily => self
                .material_family
                .map(|f| f.label().into())
                .unwrap_or_default(),
            Column::MaterialGrade => self.material_grade.clone(),
            Column::MaterialCondition => self.material_condition.clone(),
            Column::Density => self.density_kg_m3.map(number).unwrap_or_default(),
            Column::EngineeringProperty { property } => self
                .engineering_properties
                .get(*property)
                .map(|v| {
                    let factor = match property {
                        PropertyKey::YoungsModulusPa | PropertyKey::ShearModulusPa => 1e-9,
                        PropertyKey::YieldStrengthPa | PropertyKey::TensileStrengthPa => 1e-6,
                        PropertyKey::ThermalExpansionPerK
                        | PropertyKey::ElectricalResistivityOhmM => 1e6,
                        PropertyKey::ElongationRatio => 100.0,
                        _ => 1.0,
                    };
                    let basis = match v.provenance.basis {
                        PropertyBasis::Typical => "typical",
                        PropertyBasis::Minimum => "minimum",
                        PropertyBasis::Maximum => "maximum",
                        PropertyBasis::UserDefined => "user-defined",
                    };
                    format!("{} ({basis})", number(v.value * factor))
                })
                .unwrap_or_default(),
            Column::FinishCategory => self.finish_category.clone(),
            Column::FinishDescription => self.finish_description.clone(),
            Column::FinishProcess => requirements.map(|r| r.process.clone()).unwrap_or_default(),
            Column::FinishSpecification => requirements
                .map(|r| r.specification.clone())
                .unwrap_or_default(),
            Column::FinishDesignation => requirements
                .map(|r| r.designation.clone())
                .unwrap_or_default(),
            Column::CoatingThickness => requirements
                .and_then(|r| r.coating_thickness_um)
                .map(number)
                .unwrap_or_default(),
            Column::SurfaceRoughness => requirements
                .and_then(|r| r.surface_roughness_ra_um)
                .map(number)
                .unwrap_or_default(),
            Column::FinishNotes => requirements.map(|r| r.notes.clone()).unwrap_or_default(),
            Column::FaceFinishes => self.face_finishes.join("; "),
        }
    }
}
fn number(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    if value == 0.0 {
        return "0".into();
    }
    if value != 0.0 && (value.abs() < 1e-5 || value.abs() >= 1e10) {
        return format!("{value:.6e}");
    }
    let value = format!("{value:.6}");
    value
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

pub fn project_material<'a>(document: &'a Document, material: Option<&Id>) -> Option<&'a Material> {
    material.and_then(|id| document.materials.get(id))
}
pub fn project_finish<'a>(document: &'a Document, finish: Option<&Id>) -> Option<&'a Finish> {
    finish.and_then(|id| document.finishes.get(id))
}
