use super::{context::write_prefix_label, macros::SummarizeValues};
use std::marker::PhantomData;

pub trait MetricWrite {
    fn write_meta(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "# HELP ")?;
        write_prefix_label(self, f)?;
        write!(f, " ")?;
        self.write_help(f)?;
        writeln!(f)?;

        write!(f, "# TYPE ")?;
        write_prefix_label(self, f)?;
        writeln!(f, " {ty}", ty = self.metric_type())?;

        Ok(())
    }
    fn metric_name(&self) -> &str;
    fn metric_type(&self) -> Type;
    fn write_help(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Gauge,
    // TODO - any Counters?  likely no, since all zpool numbers can be reset
}
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Type::Gauge => "GAUGE",
        };
        write!(f, "{label}")
    }
}

pub const fn metric(metric_name: &'static str, help: &'static str) -> SimpleMetric {
    SimpleMetric {
        metric_name,
        help,
        ty: Type::Gauge,
    }
}
impl SimpleMetric {
    pub const fn with_values<T: SummarizeValues>(self) -> ValuesMetric<T> {
        ValuesMetric {
            base: self,
            _values_marker: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleMetric {
    metric_name: &'static str,
    help: &'static str,
    ty: Type,
}
impl MetricWrite for SimpleMetric {
    fn metric_name(&self) -> &str {
        self.metric_name
    }
    fn metric_type(&self) -> Type {
        self.ty
    }
    fn write_help(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { help, .. } = self;
        write!(f, "{help}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValuesMetric<T> {
    base: SimpleMetric,
    _values_marker: PhantomData<T>,
}
impl<T> MetricWrite for ValuesMetric<T>
where
    T: SummarizeValues,
{
    fn metric_name(&self) -> &str {
        self.base.metric_name()
    }
    fn metric_type(&self) -> Type {
        self.base.metric_type()
    }
    fn write_help(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.base.write_help(f)?;
        write!(f, ": ")?;
        T::summarize_values(f)
    }
}
