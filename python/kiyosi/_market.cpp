#include "_binding.hpp"

#include <chrono>

using namespace nb::literals;

namespace kiyosi::python_binding {

void bind_enums(nb::module_& module)
{
    nb::enum_<error_category>(module, "ErrorCategory")
        .value("INVALID_OPTION", error_category::invalid_option)
        .value("INVALID_STRIKE", error_category::invalid_strike)
        .value("INVALID_VOLATILITY", error_category::invalid_volatility)
        .value("INVALID_RATE", error_category::invalid_rate)
        .value("INVALID_DIVIDEND", error_category::invalid_dividend)
        .value("INVALID_ASSET_PRICE", error_category::invalid_asset_price)
        .value("INVALID_DATE", error_category::invalid_date)
        .value("INVALID_EXPIRY", error_category::invalid_expiry)
        .value("INVALID_RESULT", error_category::invalid_result)
        .value("INVALID_SCHEDULE", error_category::invalid_schedule)
        .value("INVALID_CALENDAR", error_category::invalid_calendar)
        .value("INVALID_PARAMETER", error_category::invalid_parameter)
        .value("UNBRACKETED_VOLATILITY", error_category::unbracketed_volatility)
        .value("SOLVER_NON_CONVERGENCE", error_category::solver_non_convergence)
        .value("SOLVER_NON_FINITE", error_category::solver_non_finite)
        .value("UNBRACKETED_COUPON", error_category::unbracketed_coupon);
    nb::enum_<option_type>(module, "OptionType")
        .value("CALL", option_type::call)
        .value("PUT", option_type::put);
    nb::enum_<barrier_type>(module, "BarrierType")
        .value("UP_AND_IN", barrier_type::up_and_in)
        .value("UP_AND_OUT", barrier_type::up_and_out)
        .value("DOWN_AND_IN", barrier_type::down_and_in)
        .value("DOWN_AND_OUT", barrier_type::down_and_out);
    nb::enum_<observation_mode>(module, "ObservationMode")
        .value("CONTINUOUS", observation_mode::continuous)
        .value("SCHEDULED", observation_mode::scheduled);
    nb::enum_<rebate_timing>(module, "RebateTiming")
        .value("AT_HIT", rebate_timing::at_hit)
        .value("AT_EXPIRY", rebate_timing::at_expiry);
    nb::enum_<settlement_timing>(module, "SettlementTiming")
        .value("AT_HIT", settlement_timing::at_hit)
        .value("AT_EXPIRY", settlement_timing::at_expiry);
    nb::enum_<payoff_type>(module, "PayoffType")
        .value("CASH", payoff_type::cash)
        .value("ASSET", payoff_type::asset);
    nb::enum_<observation_frequency>(module, "ObservationFrequency")
        .value("DAILY", observation_frequency::daily)
        .value("AT_EXPIRY", observation_frequency::at_expiry);
    nb::enum_<barrier_touch_status>(module, "BarrierTouchStatus")
        .value("NONE", barrier_touch_status::none)
        .value("UP", barrier_touch_status::up)
        .value("DOWN", barrier_touch_status::down);
    nb::enum_<finite_difference_scheme>(module, "FiniteDifferenceScheme")
        .value("EXPLICIT_EULER", finite_difference_scheme::explicit_euler)
        .value("IMPLICIT_EULER", finite_difference_scheme::implicit_euler)
        .value("CRANK_NICOLSON", finite_difference_scheme::crank_nicolson);
    nb::enum_<risk_measure>(module, "RiskMeasure")
        .value("PRICE", risk_measure::price)
        .value("DELTA", risk_measure::delta)
        .value("GAMMA", risk_measure::gamma)
        .value("SPEED", risk_measure::speed)
        .value("THETA", risk_measure::theta)
        .value("CHARM", risk_measure::charm)
        .value("COLOR", risk_measure::color)
        .value("VEGA", risk_measure::vega)
        .value("VANNA", risk_measure::vanna)
        .value("ZOMMA", risk_measure::zomma)
        .value("RHO", risk_measure::rho);
}

void bind_market(nb::module_& module)
{
    auto parameters = nb::class_<BsmParameters>(
        module, "BsmParameters", "Validated Black-Scholes-Merton market parameters.")
        .def(nb::new_([](PythonReal risk_free_rate, PythonReal dividend_yield,
                        PythonReal volatility) {
                 return unwrap(make_bsm_parameters(
                     real_number(risk_free_rate, "risk_free_rate"),
                     real_number(dividend_yield, "dividend_yield"),
                     real_number(volatility, "volatility")));
             }),
             nb::kw_only(), "risk_free_rate"_a, "dividend_yield"_a, "volatility"_a,
             "Create validated continuously compounded rates and volatility.")
        .def_prop_ro("risk_free_rate", &BsmParameters::risk_free_rate)
        .def_prop_ro("dividend_yield", &BsmParameters::dividend_yield)
        .def_prop_ro("volatility", &BsmParameters::volatility);
    bind_value_equality(parameters);
    bind_repr(parameters, "BsmParameters",
              {{"risk_free_rate", "risk_free_rate"},
               {"dividend_yield", "dividend_yield"},
               {"volatility", "volatility"}});

    auto calendar = nb::class_<TradingCalendar>(
        module, "TradingCalendar",
        "Read-only trading-day calendar. Instances are created by calendar factories.")
        .def("is_trading_day",
             [](const TradingCalendar& calendar, PythonDate value) {
                 return calendar.is_trading_day(calendar_date(value, "value"));
             },
             "value"_a, "Return whether value is a trading day.")
        .def("trading_days_between",
             [](const TradingCalendar& calendar, PythonDate start, PythonDate end) {
                 return unwrap(calendar.trading_days_between(
                     calendar_date(start, "start"), calendar_date(end, "end")));
             },
             "start"_a, "end"_a,
             "Count trading days in [start, end); reversed ranges are rejected.")
        .def("trading_year_fraction",
             [](const TradingCalendar& calendar, PythonDate start, PythonDate end) {
                 return unwrap(calendar.trading_year_fraction(
                     calendar_date(start, "start"), calendar_date(end, "end")));
             },
             "start"_a, "end"_a,
             "Return trading days in [start, end) divided by annual_trading_days.")
        .def_prop_ro("annual_trading_days", &TradingCalendar::annual_trading_days);
    bind_repr(calendar, "TradingCalendar",
              {{"annual_trading_days", "annual_trading_days"}});

    auto schedule = nb::class_<ObservationSchedule>(
        module, "ObservationSchedule", "Immutable ordered observation dates.")
        .def("__len__", &ObservationSchedule::size)
        .def("__getitem__", [](const ObservationSchedule& schedule, nb::ssize_t index) {
            const auto size = static_cast<nb::ssize_t>(schedule.size());
            if (index < 0) index += size;
            if (index < 0 || index >= size) throw nb::index_error();
            return python_date(schedule[static_cast<std::size_t>(index)]);
        })
        .def("__iter__", [](const ObservationSchedule& schedule) {
            PythonDateList output;
            for (const date value : schedule.dates()) output.append(python_date(value));
            return PythonDateIterator{output.attr("__iter__")()};
        })
        .def_prop_ro("dates", [](const ObservationSchedule& schedule) {
            PythonDateList output;
            for (const date value : schedule.dates()) output.append(python_date(value));
            return output;
        });
    bind_value_equality(schedule);
    bind_repr(schedule, "ObservationSchedule", {{"dates", "dates"}});

    auto context = nb::class_<PricingContext>(
        module, "PricingContext", "Validated market state for a valuation instant.")
        .def(nb::new_([](const BsmParameters& parameters, PythonReal asset_price,
                        PythonValuationTime time, const TradingCalendar& calendar) {
                 return unwrap(make_pricing_context(
                     parameters, real_number(asset_price, "asset_price"),
                     valuation_time(time), calendar));
             }),
             nb::kw_only(), "parameters"_a, "asset_price"_a, "valuation_time"_a,
             "calendar"_a = weekdays_calendar(),
             "Create a pricing context; dates denote midnight UTC and use the weekdays calendar by default.")
        .def_prop_ro("parameters", &PricingContext::parameters,
                     nb::rv_policy::reference_internal,
                     "Read-only parameters view that keeps this context alive; concurrent reads are safe.")
        .def_prop_ro("asset_price", &PricingContext::asset_price)
        .def_prop_ro("valuation_date", [](const PricingContext& context) {
            return python_date(context.valuation_date());
        })
        .def_prop_ro("valuation_time", [](const PricingContext& context) {
            return python_timestamp(context.valuation_time());
        })
        .def_prop_ro("calendar", &PricingContext::calendar,
                     nb::rv_policy::reference_internal,
                     "Read-only calendar view that keeps this context alive; concurrent reads are safe.");
    bind_repr(context, "PricingContext",
              {{"parameters", "parameters"}, {"asset_price", "asset_price"},
               {"valuation_time", "valuation_time"}, {"calendar", "calendar"}});

    module.def("all_days_calendar", &all_days_calendar,
               "Return a 365-day calendar in which every day is a trading day.");
    module.def("weekdays_calendar", &weekdays_calendar,
               "Return a holiday-unaware Monday-to-Friday calendar with 252 annual trading days.");
    module.def("sse_calendar", &sse_calendar,
               "Return the Shanghai Stock Exchange holiday calendar with 252 annual trading days.");
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
        "Build dates at a fixed calendar-day interval, adjusted to trading days.");
    module.def(
        "monthly_schedule",
        [](PythonDate start, PythonDate end, PythonInteger lock_up_months,
           const TradingCalendar& calendar) {
            return unwrap(make_monthly_schedule(
                calendar_date(start, "start"), calendar_date(end, "end"),
                integer(lock_up_months, "lock_up_months"), calendar));
        },
        nb::kw_only(), "start"_a, "end"_a, "lock_up_months"_a,
        "calendar"_a = weekdays_calendar(),
        "Build a monthly observation schedule after the lock-up period.");
}

} // namespace kiyosi::python_binding
