/// Defines the enum with a static field `ALL` containing all variants (in declaration order)
macro_rules! enum_all {
    (
        $(
            $(#[$meta:meta])*
            $vis:vis enum $name:ident {
                $(
                    $(#[$meta_inner:meta])*
                    $variant:ident $(= $variant_value:expr)?
                ),+ $(,)?
            }
        )+
    ) => {
        $(
            $(#[$meta])*
            $vis enum $name {
                $(
                    $(#[$meta_inner])*
                    $variant $(= $variant_value)?
                ),+
            }
            impl $name {
                const ALL: &'static [Self] = &[
                    $(Self::$variant,)+
                ];
            }
        )+
    };
}

/// Defines the enum with:
/// - `fn summarize_values()` to list the name/value pairs, and
/// - `fn value()` to retrieve the value
macro_rules! value_enum {
    (
        $(
            $(#[$meta:meta])*
            $vis:vis enum $name:ident for $source:ident {
                #[default]
                $UnknownMissing:ident => 0,
                $(
                    $(#[$meta_inner:meta])*
                    $variant:ident => $variant_value:expr
                ),+ $(,)?
            }
        )+
    ) => {
        $(
            enum_all! {
                #[derive(Clone, Copy, Debug, Default)]
                $(#[$meta])*
                $vis enum $name {
                    #[default]
                    $UnknownMissing = 0,
                    $(
                        $(#[$meta_inner])*
                        $variant = $variant_value
                    ),+
                }
            }
            impl $name {
                /// Returns a comma-separated representation of all variants: "Variant = value"
                #[allow(clippy::must_use_candidate)]
                pub fn summarize_values() -> impl std::fmt::Display {
                    struct Summary;
                    impl std::fmt::Display for Summary {
                        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                            let mut first = Some(());
                            for &status in $name::ALL {
                                if first.take().is_none() {
                                    write!(f, ", ")?;
                                }
                                let status_num = status.value();
                                write!(f, "{status:?} = {status_num}")?;
                            }
                            Ok(())
                        }
                    }
                    Summary
                }
                /// Returns the value from the specified `Option`
                pub fn from_opt<T>(source: &Option<T>) -> u32
                where
                    Self: From<T>,
                    T: Copy,
                {
                    source.map(Self::from).unwrap_or_default().value()
                }
                #[allow(clippy::must_use_candidate, missing_docs)]
                pub fn value(self) -> u32 {
                    match self {
                        Self::$UnknownMissing => 0,
                        $(Self::$variant => $variant_value),+
                    }
                }
            }
            impl From<$source> for $name {
                fn from(source: $source) -> Self {
                    match source {
                        $(
                            $source::$variant => Self::$variant
                        ),+
                    }
                }
            }
            impl<T> From<($source, T)> for $name {
                fn from((source, _): ($source, T)) -> Self {
                    source.into()
                }
            }
        )+
    };
}
