//! `SeriesSettings`: what the Graph view plots. Plain data; the plotting is
//! `ui::graph_view` and the arithmetic is `tools::graph::series`.

use super::store::DatasetId;

/// What trend is removed from the plotted series. (afni-core's
/// `signal::Detrend` does the arithmetic; this is the serializable choice.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Detrend {
    /// Nothing: the values as stored.
    #[default]
    None,
    /// Subtract the mean.
    Mean,
    /// Subtract the least-squares line.
    Linear,
    /// Subtract the least-squares quadratic.
    Quadratic,
}

impl Detrend {
    /// Every choice, in menu order.
    pub const ALL: [Detrend; 4] = [
        Detrend::None,
        Detrend::Mean,
        Detrend::Linear,
        Detrend::Quadratic,
    ];

    /// Menu text.
    pub fn label(self) -> &'static str {
        match self {
            Detrend::None => "none",
            Detrend::Mean => "mean",
            Detrend::Linear => "linear",
            Detrend::Quadratic => "quadratic",
        }
    }
}

/// A stimulus (or any on/off regressor) over the time points, drawn as shaded
/// blocks behind the series.
#[derive(Debug, Clone, PartialEq)]
pub struct Stim {
    /// Where it came from (a file name), for the card.
    pub name: String,
    /// One shaded set of blocks per regressor, each in its own color.
    pub conditions: Vec<Condition>,
}

/// One on/off regressor of a [`Stim`].
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    /// Its label (a `.1D` column label, or `#1`, `#2`, …).
    pub label: String,
    /// On or off at each time point.
    pub on: Vec<bool>,
}

/// The colors conditions are shaded in, in order: gold first (a lone
/// regressor looks as it always has), then blue, magenta, green, red, teal.
pub const CONDITION_COLORS: [[u8; 3]; 6] = [
    [235, 175, 0],
    [40, 140, 240],
    [220, 60, 190],
    [50, 180, 80],
    [230, 70, 50],
    [20, 175, 175],
];

/// The color of the `i`th condition.
pub fn condition_color(i: usize) -> [u8; 3] {
    CONDITION_COLORS[i % CONDITION_COLORS.len()]
}

/// The Graph view's settings (per controller).
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesSettings {
    /// The dataset plotted; `None` is the underlay.
    pub source: Option<DatasetId>,
    /// A fitted time series drawn over it (same number of time points).
    pub fit: Option<DatasetId>,
    /// 1, 3 or 5: an `n × n` matrix of neighboring voxels' series.
    pub matrix: u8,
    /// Time points skipped at the start (AFNI's graph "ignore").
    pub ignore: usize,
    /// Trend removed from what is plotted.
    pub detrend: Detrend,
    /// Plot percent of the mean instead of the stored values.
    pub percent: bool,
    /// Shaded blocks, if a stimulus was loaded.
    pub stim: Option<Stim>,
}

impl Default for SeriesSettings {
    fn default() -> Self {
        Self {
            source: None,
            fit: None,
            matrix: 1,
            ignore: 0,
            detrend: Detrend::None,
            percent: false,
            stim: None,
        }
    }
}

/// A change to the settings, applied by `Session::apply`.
#[derive(Debug, Clone, PartialEq)]
pub enum SeriesChange {
    /// Plot this dataset (`None`: the underlay).
    Source(Option<DatasetId>),
    /// Draw this dataset as the fit (`None`: no fit).
    Fit(Option<DatasetId>),
    /// 1, 3 or 5.
    Matrix(u8),
    /// Skip this many time points.
    Ignore(usize),
    /// Remove a trend.
    Detrend(Detrend),
    /// Percent of the mean.
    Percent(bool),
    /// Load or clear the stimulus.
    Stim(Option<Stim>),
}

/// The most time points that can be ignored.
pub const MAX_IGNORE: usize = 10_000;
