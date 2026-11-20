use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ReportSection {
    CaseInformation,
    EvidenceSources,
    Artifacts,
    Media,
    Timeline,
    ProcessingLog,
}

impl ReportSection {
    pub fn id(self) -> &'static str {
        match self {
            Self::CaseInformation => "caseInformation",
            Self::EvidenceSources => "evidenceSources",
            Self::Artifacts => "artifacts",
            Self::Media => "media",
            Self::Timeline => "timeline",
            Self::ProcessingLog => "processingLog",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::CaseInformation => "Case information",
            Self::EvidenceSources => "Evidence sources",
            Self::Artifacts => "Artifact records",
            Self::Media => "Media inventory and stored hashes",
            Self::Timeline => "Timeline events",
            Self::ProcessingLog => "Processing history and logs",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportRequest {
    pub title: String,
    pub examiner: String,
    pub notes: String,
    pub sections: Vec<ReportSection>,
    pub datasource_ids: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportRecord {
    pub id: String,
    pub title: String,
    pub examiner: String,
    pub created_at: String,
    pub sections: Vec<ReportSection>,
    pub datasource_ids: Vec<String>,
    pub counts: Vec<SectionCount>,
    pub size_bytes: u64,
    pub sha256: String,
    pub path: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionCount {
    pub section: ReportSection,
    pub rows: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportCatalog {
    pub datasources: Vec<ReportDatasource>,
    pub sections: Vec<SectionCount>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDatasource {
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub path: String,
}

pub struct ReportCase {
    pub id: String,
    pub name: String,
    pub examiner: String,
    pub reference: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ReportLibrary {
    pub reports: Vec<ReportRecord>,
    pub warnings: Vec<String>,
}
