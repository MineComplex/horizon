//! Port of `com.mojang.serialization.DataResult` (DFU 8): the sealed interface and its two records
//! `DataResult.Success` / `DataResult.Error`, as an enum of the two record structs.
//!
//! Static factories (Java overloads by arity): `DataResult::success(r)`, `success_2(r, lifecycle)`,
//! `DataResult::error(|| msg)`, `error_2(|| msg, partial)`, `error_2_2(|| msg, lifecycle)`,
//! `error_3(|| msg, partial, lifecycle)`. Instance methods are inherent, except `error()` (the
//! `Optional<Error<R>>` accessor), which lives on the `DataResultMethods` trait because Rust can't have an
//! associated function and a method with the same name on one type (the trait is in the prelude).
use crate::com::mojang::serialization::lifecycle::Lifecycle;
use crate::prelude::*;

/// A message supplier (Java's `Supplier<String>`).
pub type MessageSupplier = Arc<dyn Fn() -> String + Send + Sync>;

// ---- record com.mojang.serialization.DataResult.Success
#[derive(Clone)]
pub struct DataResultSuccess<R> {
    pub value: R,
    pub lifecycle: Lifecycle,
}

impl<R> DataResultSuccess<R> {
    pub fn value(&self) -> R
    where
        R: Clone,
    {
        self.value.clone()
    }
    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }
}

// ---- record com.mojang.serialization.DataResult.Error
#[derive(Clone)]
pub struct DataResultError<R> {
    pub message_supplier: MessageSupplier,
    pub partial_value: Option<R>,
    pub lifecycle: Lifecycle,
}

impl<R> DataResultError<R> {
    pub fn message(&self) -> String {
        (self.message_supplier)()
    }
    pub fn message_supplier(&self) -> MessageSupplier {
        self.message_supplier.clone()
    }
    pub fn partial_value(&self) -> Option<R>
    where
        R: Clone,
    {
        self.partial_value.clone()
    }
    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }
    /// `toString()`: `DataResult.Error['msg': partial]`.
    pub fn to_string(&self) -> String
    where
        R: std::fmt::Debug,
    {
        format!(
            "DataResult.Error['{}'{}]",
            self.message(),
            self.partial_value
                .as_ref()
                .map(|v| format!(": {:?}", v))
                .unwrap_or_default()
        )
    }
}

// ---- interface com.mojang.serialization.DataResult
#[derive(Clone)]
pub enum DataResult<R> {
    Success(DataResultSuccess<R>),
    Error(DataResultError<R>),
}

fn append_messages(first: &str, second: &str) -> String {
    format!("{}; {}", first, second)
}

fn join_suppliers(first: MessageSupplier, second: MessageSupplier) -> MessageSupplier {
    Arc::new(move || append_messages(&first(), &second()))
}

impl<R> DataResult<R> {
    // ---- static factories

    pub fn success(result: R) -> Self {
        Self::success_2(result, Lifecycle::experimental())
    }

    pub fn success_2(result: R, lifecycle: Lifecycle) -> Self {
        DataResult::Success(DataResultSuccess {
            value: result,
            lifecycle,
        })
    }

    pub fn error(message: impl Fn() -> String + Send + Sync + 'static) -> Self {
        Self::error_2_2(message, Lifecycle::experimental())
    }

    pub fn error_2(
        message: impl Fn() -> String + Send + Sync + 'static,
        partial_result: R,
    ) -> Self {
        Self::error_3(message, partial_result, Lifecycle::experimental())
    }

    pub fn error_2_2(
        message: impl Fn() -> String + Send + Sync + 'static,
        lifecycle: Lifecycle,
    ) -> Self {
        DataResult::Error(DataResultError {
            message_supplier: Arc::new(message),
            partial_value: None,
            lifecycle,
        })
    }

    pub fn error_3(
        message: impl Fn() -> String + Send + Sync + 'static,
        partial_result: R,
        lifecycle: Lifecycle,
    ) -> Self {
        DataResult::Error(DataResultError {
            message_supplier: Arc::new(message),
            partial_value: Some(partial_result),
            lifecycle,
        })
    }

    /// An error from an existing supplier (Rust-only convenience).
    pub fn error_from(
        message_supplier: MessageSupplier,
        partial_value: Option<R>,
        lifecycle: Lifecycle,
    ) -> Self {
        DataResult::Error(DataResultError {
            message_supplier,
            partial_value,
            lifecycle,
        })
    }

    /// `DataResult.partialGet(partialGet, errorPrefix)`.
    pub fn partial_get<K: std::fmt::Display + 'static>(
        partial_get: impl Fn(&K) -> Option<R> + Send + Sync + 'static,
        error_prefix: impl Fn() -> String + Send + Sync + 'static,
    ) -> impl Fn(K) -> DataResult<R> {
        let error_prefix: MessageSupplier = Arc::new(error_prefix);
        move |name: K| match partial_get(&name) {
            Some(v) => DataResult::success(v),
            None => {
                let prefix = error_prefix.clone();
                let name = name.to_string();
                DataResult::error(move || format!("{}{}", prefix(), name))
            }
        }
    }

    // ---- accessors

    pub fn result(&self) -> Option<R>
    where
        R: Clone,
    {
        match self {
            DataResult::Success(s) => Some(s.value.clone()),
            DataResult::Error(_) => None,
        }
    }

    /// `result()` without cloning.
    pub fn into_result(self) -> Option<R> {
        match self {
            DataResult::Success(s) => Some(s.value),
            DataResult::Error(_) => None,
        }
    }

    pub fn lifecycle(&self) -> Lifecycle {
        match self {
            DataResult::Success(s) => s.lifecycle,
            DataResult::Error(e) => e.lifecycle,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, DataResult::Success(_))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, DataResult::Error(_))
    }

    pub fn has_result_or_partial(&self) -> bool {
        match self {
            DataResult::Success(_) => true,
            DataResult::Error(e) => e.partial_value.is_some(),
        }
    }

    /// `resultOrPartial(Consumer<String> onError)`.
    pub fn result_or_partial(&self, on_error: impl FnOnce(String)) -> Option<R>
    where
        R: Clone,
    {
        match self {
            DataResult::Success(s) => Some(s.value.clone()),
            DataResult::Error(e) => {
                on_error(e.message());
                e.partial_value.clone()
            }
        }
    }

    /// `resultOrPartial()`.
    pub fn result_or_partial_0(&self) -> Option<R>
    where
        R: Clone,
    {
        match self {
            DataResult::Success(s) => Some(s.value.clone()),
            DataResult::Error(e) => e.partial_value.clone(),
        }
    }

    /// `resultOrPartial` without cloning.
    pub fn into_result_or_partial(self) -> Option<R> {
        match self {
            DataResult::Success(s) => Some(s.value),
            DataResult::Error(e) => e.partial_value,
        }
    }

    /// `getOrThrow()`: the value, or an `IllegalStateException` (a panic) with the message.
    pub fn get_or_throw(&self) -> R
    where
        R: Clone,
    {
        match self {
            DataResult::Success(s) => s.value.clone(),
            DataResult::Error(e) => panic!("{}", e.message()),
        }
    }

    /// `getOrThrow()` without cloning.
    pub fn into_or_throw(self) -> R {
        match self {
            DataResult::Success(s) => s.value,
            DataResult::Error(e) => panic!("{}", e.message()),
        }
    }

    /// `getOrThrow(Function<String, E> exceptionSupplier)` for checked exceptions.
    pub fn get_or_throw_1<E>(&self, exception_supplier: impl FnOnce(String) -> E) -> Result<R, E>
    where
        R: Clone,
    {
        match self {
            DataResult::Success(s) => Ok(s.value.clone()),
            DataResult::Error(e) => Err(exception_supplier(e.message())),
        }
    }

    pub fn get_partial_or_throw(&self) -> R
    where
        R: Clone,
    {
        match self {
            DataResult::Success(s) => s.value.clone(),
            DataResult::Error(e) => match &e.partial_value {
                Some(v) => v.clone(),
                None => panic!("{}", e.message()),
            },
        }
    }

    pub fn get_partial_or_throw_1<E>(
        &self,
        exception_supplier: impl FnOnce(String) -> E,
    ) -> Result<R, E>
    where
        R: Clone,
    {
        match self {
            DataResult::Success(s) => Ok(s.value.clone()),
            DataResult::Error(e) => match &e.partial_value {
                Some(v) => Ok(v.clone()),
                None => Err(exception_supplier(e.message())),
            },
        }
    }

    // ---- transformations

    pub fn map<T>(self, function: impl FnOnce(R) -> T) -> DataResult<T> {
        match self {
            DataResult::Success(s) => DataResult::Success(DataResultSuccess {
                value: function(s.value),
                lifecycle: s.lifecycle,
            }),
            DataResult::Error(e) => DataResult::Error(DataResultError {
                message_supplier: e.message_supplier,
                partial_value: e.partial_value.map(function),
                lifecycle: e.lifecycle,
            }),
        }
    }

    pub fn map_or_else<T>(
        self,
        success_function: impl FnOnce(R) -> T,
        error_function: impl FnOnce(DataResultError<R>) -> T,
    ) -> T {
        match self {
            DataResult::Success(s) => success_function(s.value),
            DataResult::Error(e) => error_function(e),
        }
    }

    pub fn if_success(self, if_success: impl FnOnce(&R)) -> Self {
        if let DataResult::Success(s) = &self {
            if_success(&s.value);
        }
        self
    }

    pub fn if_error(self, if_error: impl FnOnce(&DataResultError<R>)) -> Self {
        if let DataResult::Error(e) = &self {
            if_error(e);
        }
        self
    }

    pub fn promote_partial(self, on_error: impl FnOnce(String)) -> Self {
        match self {
            DataResult::Success(_) => self,
            DataResult::Error(e) => {
                on_error(e.message());
                match e.partial_value {
                    Some(value) => DataResult::Success(DataResultSuccess {
                        value,
                        lifecycle: e.lifecycle,
                    }),
                    None => DataResult::Error(e),
                }
            }
        }
    }

    pub fn flat_map<R2>(self, function: impl FnOnce(R) -> DataResult<R2>) -> DataResult<R2> {
        match self {
            DataResult::Success(s) => function(s.value).add_lifecycle(s.lifecycle),
            DataResult::Error(e) => match e.partial_value {
                None => DataResult::Error(DataResultError {
                    message_supplier: e.message_supplier,
                    partial_value: None,
                    lifecycle: e.lifecycle,
                }),
                Some(partial) => match function(partial) {
                    DataResult::Success(s) => DataResult::Error(DataResultError {
                        message_supplier: e.message_supplier,
                        partial_value: Some(s.value),
                        lifecycle: e.lifecycle.add(s.lifecycle),
                    }),
                    DataResult::Error(e2) => DataResult::Error(DataResultError {
                        message_supplier: join_suppliers(e.message_supplier, e2.message_supplier),
                        partial_value: e2.partial_value,
                        lifecycle: e.lifecycle.add(e2.lifecycle),
                    }),
                },
            },
        }
    }

    /// `ap(DataResult<Function<R, R2>> functionResult)`.
    pub fn ap<R2, F: FnOnce(R) -> R2>(self, function_result: DataResult<F>) -> DataResult<R2> {
        match self {
            DataResult::Success(s) => match function_result {
                DataResult::Success(f) => DataResult::Success(DataResultSuccess {
                    value: (f.value)(s.value),
                    lifecycle: s.lifecycle.add(f.lifecycle),
                }),
                DataResult::Error(fe) => DataResult::Error(DataResultError {
                    message_supplier: fe.message_supplier,
                    partial_value: fe.partial_value.map(|f| f(s.value)),
                    lifecycle: s.lifecycle.add(fe.lifecycle),
                }),
            },
            DataResult::Error(e) => match function_result {
                DataResult::Success(f) => DataResult::Error(DataResultError {
                    message_supplier: e.message_supplier,
                    partial_value: e.partial_value.map(f.value),
                    lifecycle: e.lifecycle.add(f.lifecycle),
                }),
                DataResult::Error(fe) => DataResult::Error(DataResultError {
                    message_supplier: join_suppliers(e.message_supplier, fe.message_supplier),
                    partial_value: match (e.partial_value, fe.partial_value) {
                        (Some(a), Some(f)) => Some(f(a)),
                        _ => None,
                    },
                    lifecycle: e.lifecycle.add(fe.lifecycle),
                }),
            },
        }
    }

    /// `apply2(BiFunction<R, R2, S> function, DataResult<R2> second)` (`Instance.apply2`: `ap2(point(f), this, second)`;
    /// `point` is `success(f)`, experimental).
    pub fn apply2<R2, S>(
        self,
        function: impl FnOnce(R, R2) -> S,
        second: DataResult<R2>,
    ) -> DataResult<S> {
        ap2_results(DataResult::success(function), self, second)
    }

    /// `apply2stable`: the function's lifecycle is stable.
    pub fn apply2stable<R2, S>(
        self,
        function: impl FnOnce(R, R2) -> S,
        second: DataResult<R2>,
    ) -> DataResult<S> {
        ap2_results(
            DataResult::success(function).set_lifecycle(Lifecycle::stable()),
            self,
            second,
        )
    }

    /// `apply3(Function3<R, R2, R3, S> function, DataResult<R2> second, DataResult<R3> third)`.
    pub fn apply3<R2, R3, S>(
        self,
        function: impl FnOnce(R, R2, R3) -> S,
        second: DataResult<R2>,
        third: DataResult<R3>,
    ) -> DataResult<S> {
        ap3_results(DataResult::success(function), self, second, third)
    }

    /// `setPartial(Supplier<R> partial)`.
    pub fn set_partial(self, partial: impl FnOnce() -> R) -> Self {
        match self {
            DataResult::Success(_) => self,
            DataResult::Error(e) => DataResult::Error(DataResultError {
                message_supplier: e.message_supplier,
                partial_value: Some(partial()),
                lifecycle: e.lifecycle,
            }),
        }
    }

    /// `setPartial(R partial)`.
    pub fn set_partial_1(self, partial: R) -> Self {
        self.set_partial(move || partial)
    }

    pub fn map_error(self, function: impl Fn(String) -> String + Send + Sync + 'static) -> Self {
        match self {
            DataResult::Success(_) => self,
            DataResult::Error(e) => {
                let supplier = e.message_supplier;
                DataResult::Error(DataResultError {
                    message_supplier: Arc::new(move || function(supplier())),
                    partial_value: e.partial_value,
                    lifecycle: e.lifecycle,
                })
            }
        }
    }

    pub fn set_lifecycle(self, lifecycle: Lifecycle) -> Self {
        match self {
            DataResult::Success(s) => DataResult::Success(DataResultSuccess {
                value: s.value,
                lifecycle,
            }),
            DataResult::Error(e) => DataResult::Error(DataResultError {
                message_supplier: e.message_supplier,
                partial_value: e.partial_value,
                lifecycle,
            }),
        }
    }

    pub fn add_lifecycle(self, lifecycle: Lifecycle) -> Self {
        let l = self.lifecycle().add(lifecycle);
        self.set_lifecycle(l)
    }

    /// The error, if any, without its partial value (Java code passes `DataResult<?>` around for its
    /// error alone: `withErrorsFrom(result)`).
    pub fn wildcard(self) -> DataResult<JObjectRef>
    where
        R: JGeneric,
    {
        self.map(crate::com::mojang::serialization::erased::erase)
    }

    /// `toString()`.
    pub fn to_string(&self) -> String
    where
        R: std::fmt::Debug,
    {
        match self {
            DataResult::Success(s) => format!("DataResult.Success[{:?}]", s.value),
            DataResult::Error(e) => e.to_string(),
        }
    }
}

/// `DataResult.Instance.ap2(func, a, b)`: the fast path when all three succeed, else
/// `ap(ap(map(curry, func), a), b)` (messages of later arguments come first).
pub fn ap2_results<A, B, R, F: FnOnce(A, B) -> R>(
    func: DataResult<F>,
    a: DataResult<A>,
    b: DataResult<B>,
) -> DataResult<R> {
    let curried = func.map(|f| move |a1: A| move |b1: B| f(a1, b1));
    b.ap(a.ap(curried))
}

/// `DataResult.Instance.ap3(func, t1, t2, t3)`: `ap2(ap(map(curry, func), t1), t2, t3)` off the fast path.
pub fn ap3_results<A, B, C, R, F: FnOnce(A, B, C) -> R>(
    func: DataResult<F>,
    t1: DataResult<A>,
    t2: DataResult<B>,
    t3: DataResult<C>,
) -> DataResult<R> {
    let curried = func.map(|f| move |a1: A| move |b1: B, c1: C| f(a1, b1, c1));
    ap2_results(t1.ap(curried), t2, t3)
}

/// Java's `Optional<Error<R>> error()` (a method; see the module comment).
pub trait DataResultMethods<R> {
    fn error(&self) -> Option<DataResultError<R>>;
}

impl<R: Clone> DataResultMethods<R> for DataResult<R> {
    fn error(&self) -> Option<DataResultError<R>> {
        match self {
            DataResult::Success(_) => None,
            DataResult::Error(e) => Some(e.clone()),
        }
    }
}

impl<R: std::fmt::Debug> std::fmt::Debug for DataResult<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string())
    }
}
