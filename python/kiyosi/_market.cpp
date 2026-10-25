#include "_binding.hpp"

#include <chrono>

using namespace nb::literals;

namespace kiyosi::python_binding {

void bind_error_category(nb::module_& module)
{
    nb::enum_<ErrorCategory>(module, "ErrorCategory", R"doc(Stable category for a core domain error.

Attributes
----------
INVALID_OPTION
    Invalid or internally inconsistent instrument terms.
INVALID_STRIKE
    Invalid strike value.
INVALID_VOLATILITY
    Invalid volatility value.
INVALID_RISK_FREE_RATE
    Invalid risk-free rate.
INVALID_DIVIDEND_YIELD
    Invalid dividend yield.
INVALID_SPOT_PRICE
    Invalid spot price.
INVALID_DATE
    Invalid calendar date.
INVALID_TIME_RANGE
    Invalid or reversed time range.
INVALID_RESULT
    Missing or invalid pricing result.
INVALID_SCHEDULE
    Invalid observation schedule.
INVALID_CALENDAR
    Invalid trading calendar.
INVALID_PARAMETER
    Invalid numerical or engine parameter.
UNBRACKETED_VOLATILITY
    Observed price is not bracketed by the volatility bounds.
SOLVER_NON_CONVERGENCE
    A numerical solver exhausted its iteration limit.
SOLVER_NON_FINITE
    A numerical solver encountered a non-finite value.
UNBRACKETED_COUPON
    Observed price is not bracketed by the coupon bounds.
BACKEND_UNAVAILABLE
    Requested computation backend is unavailable.
BACKEND_FAILURE
    Requested computation backend failed.
UNSUPPORTED_OPERATION
    Engine or instrument does not support the requested operation.)doc")
        .value("INVALID_OPTION", ErrorCategory::invalid_option)
        .value("INVALID_STRIKE", ErrorCategory::invalid_strike)
        .value("INVALID_VOLATILITY", ErrorCategory::invalid_volatility)
        .value("INVALID_RISK_FREE_RATE", ErrorCategory::invalid_risk_free_rate)
        .value("INVALID_DIVIDEND_YIELD", ErrorCategory::invalid_dividend_yield)
        .value("INVALID_SPOT_PRICE", ErrorCategory::invalid_spot_price)
        .value("INVALID_DATE", ErrorCategory::invalid_date)
        .value("INVALID_TIME_RANGE", ErrorCategory::invalid_time_range)
        .value("INVALID_RESULT", ErrorCategory::invalid_result)
        .value("INVALID_SCHEDULE", ErrorCategory::invalid_schedule)
        .value("INVALID_CALENDAR", ErrorCategory::invalid_calendar)
        .value("INVALID_PARAMETER", ErrorCategory::invalid_parameter)
        .value("UNBRACKETED_VOLATILITY", ErrorCategory::unbracketed_volatility)
        .value("SOLVER_NON_CONVERGENCE", ErrorCategory::solver_non_convergence)
        .value("SOLVER_NON_FINITE", ErrorCategory::solver_non_finite)
        .value("UNBRACKETED_COUPON", ErrorCategory::unbracketed_coupon)
        .value("BACKEND_UNAVAILABLE", ErrorCategory::backend_unavailable)
        .value("BACKEND_FAILURE", ErrorCategory::backend_failure)
        .value("UNSUPPORTED_OPERATION", ErrorCategory::unsupported_operation);
}

void bind_market(nb::module_& module)
{
    const auto omitted = nb::module_::import_("builtins").attr("object")();
    auto curve = nb::class_<RateTermStructure>(module, "RateTermStructure", R"doc(Immutable owning rate curve with Actual/365 Fixed time.

Node factors are log-linearly interpolated from an implicit factor of one at
reference_time. Nodes must increase strictly after the reference and factors
must be finite and positive. Factors may exceed one. Extrapolation is rejected.
Concurrent reads are safe; returned node lists and factors are owning copies.)doc");
    curve.def(nb::new_([](PythonValuationTime reference_time, PythonTimeSequence times, PythonRealSequence factors) {
                  std::vector<Timestamp> nodes;
                  for (const nb::handle time : nb::borrow<nb::iterable>(times))
                      nodes.push_back(valuation_time(time));
                  return unwrap(make_rate_term_structure(valuation_time(reference_time), std::move(nodes), real_sequence(factors, "factors")));
              }),
              nb::kw_only(), "reference_time"_a, "times"_a, "factors"_a,
              "Create a dated factor curve; dates mean midnight UTC and datetimes must be timezone-aware.");
    curve.def_static("flat", [](PythonReal rate, PythonValuationTime reference_time) { return unwrap(make_flat_rate(real_number(rate, "rate"), reference_time.is_none() ? std::nullopt : std::optional{valuation_time(reference_time)})); }, "rate"_a, nb::kw_only(), "reference_time"_a = nb::none(), "Create a finite continuous flat rate, optionally restricted by a reference timestamp.");
    curve.def_prop_ro("reference_time", [](const RateTermStructure& self) -> std::optional<PythonTimestampObject> {
        if (const auto time = self.reference_time()) return python_timestamp(*time);
        return std::nullopt;
    });
    curve.def_prop_ro("max_time", [](const RateTermStructure& self) -> std::optional<PythonTimestampObject> {
        if (const auto time = self.max_time()) return python_timestamp(*time);
        return std::nullopt;
    });
    curve.def_prop_ro("is_flat", &RateTermStructure::is_flat);
    curve.def_prop_ro("nodes", [](const RateTermStructure& self) {
        nb::typed<nb::list, PythonTimestampAnnotation> times;
        for (const Timestamp time : self.nodes())
            times.append(python_timestamp(time));
        return times;
    });
    curve.def_prop_ro("factors", &RateTermStructure::factors);
    curve.def("factor", [](const RateTermStructure& self, PythonValuationTime start, PythonValuationTime end) { return unwrap(self.factor(valuation_time(start), valuation_time(end))); }, "start"_a, "end"_a, "Return the interval factor; covered, ordered timestamps are required. Underflow returns zero.");
    curve.def("log_factor", [](const RateTermStructure& self, PythonValuationTime start, PythonValuationTime end) { return unwrap(self.log_factor(valuation_time(start), valuation_time(end))); }, "start"_a, "end"_a, "Return the interval log factor without exponentiation.");
    curve.def("forward_rate", [](const RateTermStructure& self, PythonValuationTime time) { return unwrap(self.forward_rate(valuation_time(time))); }, "time"_a, "Return the right-hand forward rate, or the left interval at the final node.");
    curve.def("shifted", [](const RateTermStructure& self, PythonReal shift) { return unwrap(self.shifted(real_number(shift, "shift"))); }, "shift"_a, "Return an owning copy with a parallel continuously compounded rate shift.");
    bind_value_equality(curve);
    bind_repr(curve, "RateTermStructure", {"reference_time", "is_flat", "nodes", "factors"});

    const auto rate_input = [](nb::handle value, std::string_view field) -> std::variant<double, RateTermStructure> {
        if (nb::isinstance<RateTermStructure>(value)) return nb::cast<RateTermStructure>(value);
        return real_number(value, field);
    };
    auto parameters = nb::class_<BlackScholesMertonParameters>(
                          module, "BlackScholesMertonParameters", R"doc(Immutable validated Black-Scholes-Merton market parameters.)doc")
                          .def(nb::new_([rate_input](PythonRate risk_free_rate, PythonRate dividend_yield,
                                                     PythonReal volatility) {
                                   const auto risk = rate_input(risk_free_rate, "risk_free_rate");
                                   const auto dividend = rate_input(dividend_yield, "dividend_yield");
                                   const auto sigma = real_number(volatility, "volatility");
                                   return std::visit([sigma](const auto& risk_value, const auto& dividend_value) {
                                       return unwrap(make_bsm_parameters(risk_value, dividend_value, sigma));
                                   },
                                                     risk, dividend);
                               }),
                               nb::kw_only(), "risk_free_rate"_a, "dividend_yield"_a, "volatility"_a,
                               R"doc(Create validated Black-Scholes-Merton parameters.

Parameters
----------
risk_free_rate : float or RateTermStructure
    Continuous annual risk-free rate or owning discount-factor curve.
dividend_yield : float or RateTermStructure
    Continuous annual dividend yield or owning continuous-yield factor curve.
volatility : float
    Positive annualized volatility.

Raises
------
TypeError
    If a rate is neither a curve nor a real number, or volatility is not real.
OverflowError
    If a value cannot be represented as a C++ ``double``.
KiyosiError
    If a value is non-finite or volatility is not positive.)doc")
                          .def_prop_ro("risk_free_rate", [](const BlackScholesMertonParameters& self) -> PythonRateValue {
                              if (const auto rate = self.risk_free_curve().flat_rate()) return nb::float_(*rate);
                              return nb::cast(self.risk_free_curve(), nb::rv_policy::copy); }, "Flat annual risk-free rate or an owning node-based curve.")
                          .def_prop_ro("dividend_yield", [](const BlackScholesMertonParameters& self) -> PythonRateValue {
                              if (const auto rate = self.dividend_curve().flat_rate()) return nb::float_(*rate);
                              return nb::cast(self.dividend_curve(), nb::rv_policy::copy); }, "Flat annual dividend yield or an owning node-based curve.")
                          .def_prop_ro("risk_free_curve", &BlackScholesMertonParameters::risk_free_curve, nb::rv_policy::copy)
                          .def_prop_ro("dividend_curve", &BlackScholesMertonParameters::dividend_curve, nb::rv_policy::copy)
                          .def_prop_ro("volatility", &BlackScholesMertonParameters::volatility, "Positive annualized volatility.");
    parameters.def("replace", [omitted](const BlackScholesMertonParameters& self, PythonRate risk_free_rate, PythonRate dividend_yield, PythonReal volatility) {
        BlackScholesMertonParameterChanges changes;
        if (!risk_free_rate.is(omitted)) {
            if (nb::isinstance<RateTermStructure>(risk_free_rate)) changes.risk_free_curve = nb::cast<RateTermStructure>(risk_free_rate);
            else changes.risk_free_rate = real_number(risk_free_rate, "risk_free_rate");
        }
        if (!dividend_yield.is(omitted)) {
            if (nb::isinstance<RateTermStructure>(dividend_yield)) changes.dividend_curve = nb::cast<RateTermStructure>(dividend_yield);
            else changes.dividend_yield = real_number(dividend_yield, "dividend_yield");
        }
        if (!volatility.is(omitted)) changes.volatility = real_number(volatility, "volatility");
        return unwrap(self.replace(changes)); }, nb::kw_only(), "risk_free_rate"_a.sig("...") = omitted, "dividend_yield"_a.sig("...") = omitted, "volatility"_a.sig("...") = omitted,
                   R"doc(Return a validated copy with the supplied fields replaced.

Omitted fields retain their current values; no arguments returns an equal value.
Arguments use the constructor's units, conversion rules and domain validation.
Explicit None and unknown fields raise TypeError. Domain errors raise KiyosiError.
The original parameters remain unchanged on success or failure.)doc");
    bind_value_equality(parameters);
    bind_repr(parameters, "BlackScholesMertonParameters",
              {"risk_free_rate",
               "dividend_yield",
               "volatility"});

    auto calendar = nb::class_<TradingCalendar>(
                        module, "TradingCalendar",
                        R"doc(Read-only trading-day calendar.

Instances are created by :func:`all_days_calendar`, :func:`weekdays_calendar`,
:func:`sse_calendar`, or :func:`custom_calendar`.)doc")
                        .def("is_trading_day", [](const TradingCalendar& calendar, PythonDate value) { return calendar.is_trading_day(calendar_date(value, "value")); }, "value"_a, R"doc(Return whether a date is a trading day.

Parameters
----------
value : datetime.date
    Calendar date to inspect.

Returns
-------
bool
    ``True`` when the date is open for trading.

Raises
------
TypeError
    If ``value`` is not a :class:`datetime.date`.)doc")
                        .def("adjust", [](const TradingCalendar& calendar, PythonDate nominal, PythonChoice<BusinessDayConvention> convention) { return python_date(unwrap(calendar.adjust(calendar_date(nominal, "nominal"), convention))); }, "nominal"_a, "convention"_a, "Adjust a nominal date to a trading day using ``'following'`` or ``'preceding'``.")
                        .def("trading_days_between", [](const TradingCalendar& calendar, PythonDate start, PythonDate end) { return unwrap(calendar.trading_days_between(
                                                                                                                                 calendar_date(start, "start"), calendar_date(end, "end"))); }, "start"_a, "end"_a, R"doc(Count trading days in the half-open interval ``[start, end)``.

Parameters
----------
start : datetime.date
    First date included in the interval.
end : datetime.date
    Exclusive interval end.

Returns
-------
int
    Number of trading days.

Raises
------
TypeError
    If either argument is not a :class:`datetime.date`.
KiyosiError
    If the range is reversed.)doc")
                        .def("trading_year_fraction", [](const TradingCalendar& calendar, PythonDate start, PythonDate end) { return unwrap(calendar.trading_year_fraction(
                                                                                                                                  calendar_date(start, "start"), calendar_date(end, "end"))); }, "start"_a, "end"_a, R"doc(Return the trading-year fraction for ``[start, end)``.

Parameters
----------
start : datetime.date
    First date included in the interval.
end : datetime.date
    Exclusive interval end.

Returns
-------
float
    Trading-day count divided by :attr:`trading_days_per_year`.

Raises
------
TypeError
    If either argument is not a :class:`datetime.date`.
KiyosiError
    If the range is reversed.)doc")
                        .def_prop_ro("trading_days_per_year", &TradingCalendar::trading_days_per_year, "Annualization denominator for trading-year fractions.");
    bind_repr(calendar, "TradingCalendar",
              {"trading_days_per_year"});

    auto schedule = nb::class_<ObservationSchedule>(
                        module, "ObservationSchedule", R"doc(Immutable ordered observation dates.

The sequence supports ``len(schedule)``, integer indexing, negative indexing,
and lazy iteration. Iterators retain the immutable schedule until exhausted.)doc")
                        .def("__len__", &ObservationSchedule::size)
                        .def("__getitem__", [](const ObservationSchedule& schedule, nb::ssize_t index) {
                            const auto size = static_cast<nb::ssize_t>(schedule.size());
                            if (index < 0) index += size;
                            if (index < 0 || index >= size) throw nb::index_error();
                            return python_date(schedule[static_cast<std::size_t>(index)]);
                        })
                        .def("__iter__", [](nb::handle schedule) {
                            auto iterator = nb::steal<PythonDateIterator>(PySeqIter_New(schedule.ptr()));
                            if (!iterator.is_valid()) throw nb::python_error();
                            return iterator;
                        })
                        .def_prop_ro("dates", [](const ObservationSchedule& schedule) { return python_dates(schedule.dates()); }, "Copy of the ordered observation dates.");
    bind_value_equality(schedule);
    bind_repr(schedule, "ObservationSchedule", {"dates"});

    auto context = nb::class_<PricingContext>(
                       module, "PricingContext", R"doc(Immutable validated market state for a valuation instant.)doc")
                       .def(nb::new_([](const BlackScholesMertonParameters& parameters, PythonReal spot_price,
                                        PythonValuationTime time, const TradingCalendar& calendar) {
                                return unwrap(make_pricing_context(
                                    parameters, real_number(spot_price, "spot_price"),
                                    valuation_time(time), calendar));
                            }),
                            nb::kw_only(), "model_parameters"_a, "spot_price"_a, "valuation_time"_a,
                            "calendar"_a = weekdays_calendar(),
                            R"doc(Create a validated pricing context.

Parameters
----------
model_parameters : BlackScholesMertonParameters
    Black-Scholes-Merton market parameters.
spot_price : float
    Positive underlying spot price.
valuation_time : datetime.date or datetime.datetime
    Valuation instant. Dates denote midnight UTC; datetimes must be timezone
    aware and are normalized to UTC. Date and datetime subclasses use their
    stored built-in components and timezone conversion.
calendar : TradingCalendar, optional
    Trading calendar. Defaults to :func:`weekdays_calendar`.

Raises
------
TypeError
    If an argument has an incompatible representation or a datetime is naive.
KiyosiError
    If the core rejects the spot price or valuation state.)doc")
                       .def_prop_ro("model_parameters", &PricingContext::model_parameters,
                                    nb::rv_policy::reference_internal,
                                    "Read-only parameters view that keeps this context alive; concurrent reads are safe.")
                       .def_prop_ro("spot_price", &PricingContext::spot_price,
                                    "Positive underlying spot price.")
                       .def_prop_ro("valuation_date", [](const PricingContext& context) { return python_date(context.valuation_date()); }, "UTC date containing the valuation instant.")
                       .def_prop_ro("valuation_time", [](const PricingContext& context) { return python_timestamp(context.valuation_time()); }, "Timezone-aware valuation timestamp normalized to UTC.")
                       .def_prop_ro("calendar", &PricingContext::calendar, nb::rv_policy::reference_internal, "Read-only calendar view that keeps this context alive; concurrent reads are safe.");
    context.def("replace", [omitted](const PricingContext& self, nb::typed<nb::handle, BlackScholesMertonParameters> model_parameters, PythonReal spot_price, PythonValuationTime time, nb::typed<nb::handle, TradingCalendar> calendar) {
        PricingContextChanges changes;
        if (!model_parameters.is(omitted)) {
            if (!nb::isinstance<BlackScholesMertonParameters>(model_parameters))
                type_error("model_parameters", "BlackScholesMertonParameters");
            changes.model_parameters = nb::cast<BlackScholesMertonParameters>(model_parameters);
        }
        if (!spot_price.is(omitted)) changes.spot_price = real_number(spot_price, "spot_price");
        if (!time.is(omitted)) changes.valuation_time = valuation_time(time);
        if (!calendar.is(omitted)) {
            if (!nb::isinstance<TradingCalendar>(calendar)) type_error("calendar", "TradingCalendar");
            changes.calendar = nb::cast<TradingCalendar>(calendar);
        }
        return unwrap(self.replace(changes)); }, nb::kw_only(), "model_parameters"_a.sig("...") = omitted, "spot_price"_a.sig("...") = omitted, "valuation_time"_a.sig("...") = omitted, "calendar"_a.sig("...") = omitted,
                R"doc(Return a validated copy with the supplied fields replaced.

Omitted fields retain their current values, including the calendar and exact
valuation timestamp. No arguments returns an equivalent snapshot. Arguments use
the constructor's conversion rules and domain validation, including UTC conversion.
Explicit None and unknown fields raise TypeError. Domain errors raise KiyosiError.
The original context remains unchanged on success or failure.
To change volatility, replace model_parameters with a modified parameter copy.)doc");
    bind_repr(context, "PricingContext",
              {"model_parameters", "spot_price", "valuation_time", "calendar"});

    module.def(
        "custom_calendar",
        [](const TradingCalendar& base, PythonDateSequence closed_dates, PythonDateSequence open_dates,
           PythonInteger trading_days_per_year) {
            auto closed = date_sequence(closed_dates, "closed_dates");
            auto open = date_sequence(open_dates, "open_dates");
            const auto basis = trading_days_per_year.is_none()
                                   ? std::optional<int>{}
                                   : std::optional{integer(trading_days_per_year, "trading_days_per_year")};
            return unwrap(make_custom_calendar(base, std::move(closed), std::move(open), basis));
        },
        nb::kw_only(), "base"_a, "closed_dates"_a = nb::tuple(), "open_dates"_a = nb::tuple(),
        "trading_days_per_year"_a.none() = nb::none(),
        R"doc(Create an immutable calendar by overriding dates in a base calendar.

Parameters
----------
base : TradingCalendar
    Calendar used for all dates without an override.
closed_dates, open_dates : iterable[datetime.date], optional
    Explicit closures and openings. Order is irrelevant; duplicates are removed.
    Inputs are copied, so later changes to them do not affect the calendar.
trading_days_per_year : int or None, optional
    Positive annualization basis. None inherits the base's basis; adding or removing
    trading dates never recalculates it.

Returns
-------
TradingCalendar
    Owning calendar, safe for concurrent reads and pricing without Python callbacks.

Raises
------
TypeError
    If a date is not datetime.date (datetime is rejected), or the basis is not an
    integer or None. Booleans and fractional numbers are rejected.
OverflowError
    If the basis is outside the C++ int range.
KiyosiError
    INVALID_CALENDAR for conflicting overrides or a nonpositive basis;
    INVALID_DATE for unsupported dates.)doc");

    module.def("all_days_calendar", &all_days_calendar, R"doc(Return a calendar in which every day is a trading day.

Returns
-------
TradingCalendar
    Calendar with 365 annual trading days.)doc");
    module.def("weekdays_calendar", &weekdays_calendar, R"doc(Return a holiday-unaware weekdays calendar.

Returns
-------
TradingCalendar
    Monday-to-Friday calendar with 252 annual trading days.)doc");
    module.def("sse_calendar", &sse_calendar, R"doc(Return the Shanghai Stock Exchange holiday calendar.

The bundled table uses XSHG recorded closures followed by estimated closures
through 2099. The estimate does not predict future one-off changes. Outside the
table range, queries use weekdays without exchange holidays.

Returns
-------
TradingCalendar
    Exchange calendar with 243 annual trading days.)doc");
    module.def(
        "fixed_interval_schedule",
        [](PythonDate start, PythonDate end, PythonInteger interval_days,
           const TradingCalendar& calendar) {
            return unwrap(make_fixed_interval_schedule(
                calendar_date(start, "start"), calendar_date(end, "end"),
                std::chrono::days{integer(interval_days, "interval_days")}, calendar));
        },
        nb::kw_only(), "start"_a, "end"_a, "interval_days"_a,
        "calendar"_a = weekdays_calendar(),
        R"doc(Build a fixed-calendar-day observation schedule.

Candidates are ``start + n * interval_days`` for positive ``n``. start is excluded
and ``end`` is an inclusive bound. Candidates use following
trading-day adjustment, duplicate adjusted dates are removed, and generation
stops rather than crossing ``end``; ``end`` is not guaranteed.

Parameters
----------
start : datetime.date
    Anchor date, excluded from the result.
end : datetime.date
    Inclusive upper bound for adjusted dates.
interval_days : int
    Positive number of calendar days between candidates.
calendar : TradingCalendar, optional
    Adjustment calendar. Defaults to :func:`weekdays_calendar`.

Returns
-------
ObservationSchedule
    Immutable adjusted observation dates.

Raises
------
TypeError
    If an argument has an incompatible representation.
OverflowError
    If ``interval_days`` is outside the C++ ``int`` range.
KiyosiError
    If the dates, interval, calendar, or resulting schedule are invalid.

Examples
--------
With the weekdays calendar, 2025-01-03 through 2025-01-07 at a one-day
interval produces 2025-01-06 and 2025-01-07. Supply explicit
``observation_dates`` to an instrument constructor for bespoke terminal dates.)doc");
    module.def(
        "monthly_schedule",
        [](PythonDate start, PythonDate end, PythonInteger first_observation_month,
           const TradingCalendar& calendar) {
            return unwrap(make_monthly_schedule(
                calendar_date(start, "start"), calendar_date(end, "end"),
                integer(first_observation_month, "first_observation_month"), calendar));
        },
        nb::kw_only(), "start"_a, "end"_a, "first_observation_month"_a,
        "calendar"_a = weekdays_calendar(),
        R"doc(Build a monthly schedule starting at the specified observation month.

Monthly candidates begin at ``start + first_observation_month``. ``start`` is excluded
and ``end`` is an inclusive bound. The start day is clamped to each target
month's last day, then candidates use following trading-day adjustment.
Duplicate adjusted dates are merged, as in fixed-interval schedules.
Generation stops rather than crossing ``end``, so ``end`` is not guaranteed.

Parameters
----------
start : datetime.date
    Anchor date, excluded from the result.
end : datetime.date
    Inclusive upper bound for adjusted dates.
first_observation_month : int
    Positive month offset from start to the first candidate: 1 means next month, 2 the month after.
calendar : TradingCalendar, optional
    Adjustment calendar. Defaults to :func:`weekdays_calendar`.

Returns
-------
ObservationSchedule
    Immutable adjusted observation dates.

Raises
------
TypeError
    If an argument has an incompatible representation.
OverflowError
    If ``first_observation_month`` is outside the C++ ``int`` range.
KiyosiError
    If the dates, first observation month, calendar, or resulting schedule are invalid.

Examples
--------
With the weekdays calendar, 2025-01-01 through 2025-03-01 with first observation month 1 produces only 2025-02-03; adjusting the Saturday end candidate would
cross the bound. Supply explicit ``observation_dates`` to an instrument
constructor for bespoke terminal dates.)doc");
}

} // namespace kiyosi::python_binding
