#include "_binding.hpp"

#include <string>
#include <string_view>

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

std::string touch_option_doc(const char* kind, const char* settlement_note,
                             const char* parameters, const char* rejected_terms)
{
    const char* article = std::string_view{kind}.starts_with("asset") ? "an " : "a ";
    return std::string{"Create "} + article + kind + R"doc( option.

The factory name selects an upper or lower barrier.)doc" +
           settlement_note + R"doc(
``touch_state`` describes observations before the valuation time.

Parameters
----------
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates, each anchored at 00:00 UTC.
barrier_level : float
    Positive barrier level.
)doc" + parameters +
           R"doc(observation_mode : {'continuous', 'scheduled'}, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.
touch_state : {'untouched', 'touched'} or None, optional
    History strictly before valuation. Required once prior monitoring was possible.

Returns
-------
TouchOption
    Validated immutable )doc" +
           kind + R"doc( option.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the )doc" +
           rejected_terms + ".";
}

constexpr const char* payout_doc = R"doc(payout : float
    Fixed cash payout.
)doc";
constexpr const char* settlement_doc = R"doc(settlement_timing : {'at_hit', 'at_expiry'}, optional
    Settle at the barrier hit or at expiry.
)doc";
template <typename Instrument>
void bind_common_option_properties(nb::class_<Instrument>& binding, const char* name,
                                   std::initializer_list<const char*> fields = {"option_type", "strike", "effective_date", "expiry_date"})
{
    binding.def_prop_ro("option_type", &Instrument::option_type,
                        "Call or put payoff direction.")
        .def_prop_ro("strike", &Instrument::strike, "Positive strike price.")
        .def_prop_ro("effective_date", [](const Instrument& value) { return python_date(value.effective_date()); }, "First date on which the contract is effective.")
        .def_prop_ro("expiry_date", [](const Instrument& value) { return python_date(value.expiry_date()); }, "Contract expiry at 00:00 UTC.");
    bind_value_equality(binding);
    bind_repr(binding, name, fields);
}

template <typename Instrument>
void bind_strike_option(nb::module_& module, const char* name,
                        Result<Instrument> (*factory)(OptionType, double, Date, Date),
                        const char* description, const char* constructor_doc)
{
    auto binding = nb::class_<Instrument>(module, name, description)
                       .def(nb::new_([factory](PythonChoice<OptionType> type, PythonReal strike,
                                               PythonDate effective_date, PythonDate expiry_date) {
                                return unwrap(factory(type, real_number(strike, "strike"),
                                                      calendar_date(effective_date, "effective_date"),
                                                      calendar_date(expiry_date, "expiry_date")));
                            }),
                            nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
                            constructor_doc);
    bind_common_option_properties(binding, name);
}

template <typename AverageOptionType>
void bind_average_option(
    nb::module_& module, const char* name,
    Result<AverageOptionType> (*factory)(OptionType, double, Date, Date, Date, double))
{
    auto binding = nb::class_<AverageOptionType>(
                       module, name, R"doc(Immutable validated average-price option.)doc")
                       .def(nb::new_([factory](PythonChoice<OptionType> type, PythonReal strike,
                                               PythonDate averaging_start_date, PythonDate effective_date,
                                               PythonDate expiry_date, PythonReal realized_average) {
                                return unwrap(factory(
                                    type, real_number(strike, "strike"),
                                    calendar_date(effective_date, "effective_date"),
                                    calendar_date(averaging_start_date, "averaging_start_date"), calendar_date(expiry_date, "expiry_date"),
                                    real_number(realized_average, "realized_average")));
                            }),
                            nb::kw_only(), "option_type"_a, "strike"_a, "averaging_start_date"_a, "effective_date"_a,
                            "expiry_date"_a, "realized_average"_a = default_realized_average,
                            R"doc(Create a validated average-price option.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive strike price.
averaging_start_date : datetime.date
    First date included in the averaging period. Equality with expiry denotes a single fixing.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry at 00:00 UTC.
realized_average : float, optional
    Average realized before valuation time, or zero before averaging begins.
    Geometric averaging requires a positive value after averaging starts.
    Defaults to the core-owned pre-averaging value.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the terms or date ordering.)doc")
                       .def_prop_ro("averaging_start_date", [](const AverageOptionType& value) { return python_date(value.averaging_start_date()); }, "First date included in the averaging period; equality with expiry denotes a single fixing.")
                       .def_prop_ro("realized_average", &AverageOptionType::realized_average, "Average realized before valuation, or zero before averaging begins; geometric averaging requires a positive value after averaging starts.");
    bind_common_option_properties(binding, name,
                                  {"option_type", "strike", "averaging_start_date", "effective_date", "expiry_date", "realized_average"});
}

} // namespace

void bind_instruments(nb::module_& module)
{
    const std::string cash_one_touch_doc = touch_option_doc(
        "cash one-touch", "", (std::string{payout_doc} + settlement_doc).c_str(),
        "payoff, barrier, dates, or observation schedule");
    const std::string cash_no_touch_doc = touch_option_doc(
        "cash no-touch", " No-touch payoffs settle at\nexpiry.", payout_doc,
        "payoff, barrier, dates, or observation schedule");
    const std::string asset_one_touch_doc = touch_option_doc(
        "asset one-touch", "", settlement_doc, "barrier, dates, or observation schedule");
    const std::string asset_no_touch_doc = touch_option_doc(
        "asset no-touch", " No-touch payoffs settle at\nexpiry.", "",
        "barrier, dates, or observation schedule");

    bind_strike_option(module, "EuropeanOption", &make_european_option,
                       R"doc(Immutable validated European vanilla option.

The payoff can be exercised only at expiry.)doc",
                       R"doc(Create a validated European option.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry at 00:00 UTC.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the strike or date ordering.)doc");

    bind_strike_option(module, "AmericanOption", &make_american_option,
                       R"doc(Immutable validated American vanilla option.

The payoff may be exercised from the effective date through expiry.)doc",
                       R"doc(Create a validated American option.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date : datetime.date
    First exercise date.
expiry_date : datetime.date
    Last exercise date.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the strike or date ordering.)doc");

    auto cash = nb::class_<CashOrNothingOption>(
                    module, "CashOrNothingOption", R"doc(Immutable validated cash-or-nothing digital option.)doc")
                    .def(nb::new_([](PythonChoice<OptionType> type, PythonReal strike, PythonReal payout,
                                     PythonDate effective_date, PythonDate expiry_date) {
                             return unwrap(make_cash_or_nothing_option(
                                 type, real_number(strike, "strike"), real_number(payout, "payout"),
                                 calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date")));
                         }),
                         nb::kw_only(), "option_type"_a, "strike"_a, "payout"_a, "effective_date"_a,
                         "expiry_date"_a, R"doc(Create a validated cash-or-nothing option.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive strike price.
payout : float
    Fixed cash amount paid when the option finishes in the money.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry at 00:00 UTC.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the payoff terms or date ordering.)doc")
                    .def_prop_ro("payout", &CashOrNothingOption::payout,
                                 "Fixed in-the-money cash payout.");
    bind_common_option_properties(cash, "CashOrNothingOption", {"option_type", "strike", "payout", "effective_date", "expiry_date"});

    bind_strike_option(module, "AssetOrNothingOption", &make_asset_or_nothing_option,
                       R"doc(Immutable validated asset-or-nothing digital option.)doc",
                       R"doc(Create a validated asset-or-nothing option.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry at 00:00 UTC.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the strike or date ordering.)doc");

    bind_average_option(module, "GeometricAveragePriceOption", &make_geometric_average_option);
    bind_average_option(module, "ArithmeticAveragePriceOption", &make_arithmetic_average_option);

    auto barrier = nb::class_<BarrierOption>(
                       module, "BarrierOption", R"doc(Immutable validated barrier option.

Scheduled barriers are observed only on ``observation_dates``; continuous
barriers require no schedule.)doc")
                       .def(nb::new_([](PythonChoice<OptionType> type, PythonReal strike, PythonDate effective_date,
                                        PythonDate expiry_date, PythonReal barrier, PythonChoice<BarrierType> barrier_type,
                                        PythonReal rebate, PythonChoice<RebateTiming> rebate_timing,
                                        PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates,
                                        std::optional<PythonChoice<BarrierTouchState>> touch_state) {
                                return unwrap(make_barrier_option({type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                                                                   calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                                                                   barrier_type, real_number(rebate, "rebate"), rebate_timing, observation_mode,
                                                                   date_sequence(observation_dates, "observation_dates"), touch_state}));
                            }),
                            nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
                            "barrier_level"_a, "barrier_type"_a, "rebate"_a = BarrierOptionTerms{}.rebate,
                            "rebate_timing"_a = BarrierOptionTerms{}.rebate_timing,
                            "observation_mode"_a = BarrierOptionTerms{}.observation_mode,
                            "observation_dates"_a = nb::make_tuple(),
                            "touch_state"_a = BarrierOptionTerms{}.touch_state,
                            R"doc(Create a validated barrier option.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates, each anchored at 00:00 UTC.
barrier_level : float
    Positive barrier level.
barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
    Barrier direction and knock-in or knock-out behavior.
rebate : float, optional
    Rebate amount. Uses the core default when omitted.
rebate_timing : {'at_hit', 'at_expiry'}, optional
    Time at which the rebate is paid.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring; omitted for continuous monitoring.
touch_state : {'untouched', 'touched'} or None, optional
    History strictly before valuation. Required once prior monitoring was possible.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the payoff, barrier, dates, or observation schedule.)doc")
                       .def_prop_ro("barrier_level", &BarrierOption::barrier_level, "Positive barrier level.")
                       .def_prop_ro("barrier_type", &BarrierOption::barrier_type, "Barrier direction and activation behavior.")
                       .def_prop_ro("rebate", &BarrierOption::rebate, "Barrier rebate amount.")
                       .def_prop_ro("rebate_timing", &BarrierOption::rebate_timing, "Time at which the rebate is paid.")
                       .def_prop_ro("observation_mode", &BarrierOption::observation_mode, "Continuous or scheduled monitoring mode.")
                       .def_prop_ro("touch_state", &BarrierOption::touch_state, "Barrier touch history before valuation.")
                       .def_prop_ro("observation_dates", [](const BarrierOption& value) { return python_dates(value.observation_dates()); }, "Copy of the scheduled observations at 00:00 UTC.");
    bind_common_option_properties(barrier, "BarrierOption",
                                  {"option_type", "strike", "effective_date", "expiry_date", "barrier_level", "barrier_type", "rebate", "rebate_timing", "observation_mode", "observation_dates", "touch_state"});

    auto binary_barrier = nb::class_<BinaryBarrierOption>(
                              module, "BinaryBarrierOption", R"doc(Immutable validated strike-based binary barrier option.

Instances are created by :func:`cash_binary_barrier_option` or
:func:`asset_binary_barrier_option`.)doc")
                              .def_prop_ro("barrier_level", &BinaryBarrierOption::barrier_level, "Positive barrier level.")
                              .def_prop_ro("barrier_type", &BinaryBarrierOption::barrier_type, "Barrier direction and activation behavior.")
                              .def_prop_ro("payoff_type", &BinaryBarrierOption::payoff_type, "Cash or asset delivery form.")
                              .def_prop_ro("payout", [](const BinaryBarrierOption& value) -> std::optional<double> {
            if (const auto* cash = std::get_if<CashOrNothingPayoff>(&value.payoff()))
                return cash->payout();
            return std::nullopt; }, "Cash payout, or None for an asset payoff.")
                              .def_prop_ro("observation_mode", &BinaryBarrierOption::observation_mode, "Continuous or scheduled monitoring mode.")
                              .def_prop_ro("touch_state", &BinaryBarrierOption::touch_state, "Barrier touch history before valuation.")
                              .def_prop_ro("observation_dates", [](const BinaryBarrierOption& value) { return python_dates(value.observation_dates()); }, "Copy of the scheduled observations at 00:00 UTC.");
    bind_common_option_properties(binary_barrier, "BinaryBarrierOption",
                                  {"option_type", "strike", "effective_date", "expiry_date", "barrier_level", "barrier_type", "payoff_type", "payout", "observation_mode", "observation_dates", "touch_state"});

    module.def("cash_binary_barrier_option", [](PythonChoice<OptionType> type, PythonReal strike, PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonChoice<BarrierType> barrier_type, PythonReal payout, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_cash_binary_barrier_option(
                                                                                                                                                                                                                                                                                                                                                                                            {type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                                                                                                                                                                                                                                                                                                                                                                                             calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                                                                                                                                                                                                                                                                                                                                                                                             barrier_type, observation_mode, date_sequence(observation_dates, "observation_dates"),
                                                                                                                                                                                                                                                                                                                                                                                             touch_state},
                                                                                                                                                                                                                                                                                                                                                                                            real_number(payout, "payout"))); }, nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "barrier_type"_a, "payout"_a, "observation_mode"_a = BinaryBarrierTerms{}.observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = BinaryBarrierTerms{}.touch_state,
               R"doc(Create a cash-or-nothing binary barrier option.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates, each anchored at 00:00 UTC.
barrier_level : float
    Positive barrier level.
barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
    Barrier direction and activation behavior.
payout : float
    Fixed cash amount paid when the payoff and barrier conditions hold.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.
touch_state : {'untouched', 'touched'} or None, optional
    History strictly before valuation. Required once prior monitoring was possible.

Returns
-------
BinaryBarrierOption
    Validated immutable cash binary barrier option.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the payoff, barrier, dates, or observation schedule.)doc");
    module.def("asset_binary_barrier_option", [](PythonChoice<OptionType> type, PythonReal strike, PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonChoice<BarrierType> barrier_type, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_asset_binary_barrier_option(
                                                                                                                                                                                                                                                                                                                                                                          {type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                                                                                                                                                                                                                                                                                                                                                                           calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                                                                                                                                                                                                                                                                                                                                                                           barrier_type, observation_mode, date_sequence(observation_dates, "observation_dates"),
                                                                                                                                                                                                                                                                                                                                                                           touch_state})); }, nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "barrier_type"_a, "observation_mode"_a = BinaryBarrierTerms{}.observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = BinaryBarrierTerms{}.touch_state,
               R"doc(Create an asset-or-nothing binary barrier option.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates, each anchored at 00:00 UTC.
barrier_level : float
    Positive barrier level.
barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
    Barrier direction and activation behavior.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.
touch_state : {'untouched', 'touched'} or None, optional
    History strictly before valuation. Required once prior monitoring was possible.

Returns
-------
BinaryBarrierOption
    Validated immutable asset binary barrier option.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the payoff, barrier, dates, or observation schedule.)doc");

    auto touch = nb::class_<TouchOption>(
                     module, "TouchOption", R"doc(Immutable validated one-touch or no-touch option.

Instances are created by the ``cash_*_touch_*`` and ``asset_*_touch_*``
factory functions.)doc")
                     .def_prop_ro("effective_date", [](const TouchOption& value) { return python_date(value.effective_date()); }, "First date on which the contract is effective.")
                     .def_prop_ro("expiry_date", [](const TouchOption& value) { return python_date(value.expiry_date()); }, "Contract expiry at 00:00 UTC.")
                     .def_prop_ro("barrier_level", &TouchOption::barrier_level, "Positive barrier level.")
                     .def_prop_ro("is_one_touch", &TouchOption::is_one_touch, "Whether hitting the barrier activates the payoff.")
                     .def_prop_ro("is_up", &TouchOption::is_up, "Whether the contract uses an upper barrier.")
                     .def_prop_ro("payoff_type", &TouchOption::payoff_type, "Cash or asset delivery form.")
                     .def_prop_ro("payout", [](const TouchOption& value) -> std::optional<double> {
            if (const auto* cash = std::get_if<CashOrNothingPayoff>(&value.payoff()))
                return cash->payout();
            return std::nullopt; }, "Cash payout, or None for an asset payoff.")
                     .def_prop_ro("settlement_timing", &TouchOption::settlement_timing, "Settlement time for a one-touch payoff.")
                     .def_prop_ro("observation_mode", &TouchOption::observation_mode, "Continuous or scheduled monitoring mode.")
                     .def_prop_ro("touch_state", &TouchOption::touch_state, "Barrier touch history before valuation.")
                     .def_prop_ro("observation_dates", [](const TouchOption& value) { return python_dates(value.observation_dates()); }, "Copy of the scheduled observations at 00:00 UTC.");
    bind_value_equality(touch);
    bind_repr(touch, "TouchOption",
              {"effective_date", "expiry_date", "barrier_level", "is_one_touch", "is_up", "payoff_type", "payout", "settlement_timing", "observation_mode", "observation_dates", "touch_state"});

    module.def("cash_one_touch_up", [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonReal payout, PythonChoice<SettlementTiming> settlement_timing, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_cash_one_touch_up(
                                                                                                                                                                                                                                                                                                                                           calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                                                                                                                                                                                                                                                                                                                                           real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                                                                                                                                                                                                                                                                                                                                           settlement_timing,
                                                                                                                                                                                                                                                                                                                                           observation_mode, date_sequence(observation_dates, "observation_dates"), touch_state)); }, nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a, "settlement_timing"_a = default_one_touch_settlement_timing, "observation_mode"_a = default_touch_observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = std::nullopt, cash_one_touch_doc.c_str());
    module.def("cash_one_touch_down", [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonReal payout, PythonChoice<SettlementTiming> settlement_timing, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_cash_one_touch_down(
                                                                                                                                                                                                                                                                                                                                             calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                                                                                                                                                                                                                                                                                                                                             real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                                                                                                                                                                                                                                                                                                                                             settlement_timing,
                                                                                                                                                                                                                                                                                                                                             observation_mode, date_sequence(observation_dates, "observation_dates"), touch_state)); }, nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a, "settlement_timing"_a = default_one_touch_settlement_timing, "observation_mode"_a = default_touch_observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = std::nullopt, cash_one_touch_doc.c_str());
    module.def("cash_no_touch_up", [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonReal payout, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_cash_no_touch_up(
                                                                                                                                                                                                                                                                                        calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                                                                                                                                                                                                                                                                                        real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                                                                                                                                                                                                                                                                                        observation_mode, date_sequence(observation_dates, "observation_dates"), touch_state)); }, nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a, "observation_mode"_a = default_touch_observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = std::nullopt, cash_no_touch_doc.c_str());
    module.def("cash_no_touch_down", [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonReal payout, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_cash_no_touch_down(
                                                                                                                                                                                                                                                                                          calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                                                                                                                                                                                                                                                                                          real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                                                                                                                                                                                                                                                                                          observation_mode, date_sequence(observation_dates, "observation_dates"), touch_state)); }, nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a, "observation_mode"_a = default_touch_observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = std::nullopt, cash_no_touch_doc.c_str());
    module.def("asset_one_touch_up", [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonChoice<SettlementTiming> settlement_timing, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_asset_one_touch_up(
                                                                                                                                                                                                                                                                                                                         calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                                                                                                                                                                                                                                                                                                                         real_number(barrier, "barrier_level"), settlement_timing, observation_mode,
                                                                                                                                                                                                                                                                                                                         date_sequence(observation_dates, "observation_dates"), touch_state)); }, nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "settlement_timing"_a = default_one_touch_settlement_timing, "observation_mode"_a = default_touch_observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = std::nullopt, asset_one_touch_doc.c_str());
    module.def("asset_one_touch_down", [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonChoice<SettlementTiming> settlement_timing, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_asset_one_touch_down(
                                                                                                                                                                                                                                                                                                                           calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                                                                                                                                                                                                                                                                                                                           real_number(barrier, "barrier_level"), settlement_timing, observation_mode,
                                                                                                                                                                                                                                                                                                                           date_sequence(observation_dates, "observation_dates"), touch_state)); }, nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "settlement_timing"_a = default_one_touch_settlement_timing, "observation_mode"_a = default_touch_observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = std::nullopt, asset_one_touch_doc.c_str());
    module.def("asset_no_touch_up", [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_asset_no_touch_up(
                                                                                                                                                                                                                                                                      calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                                                                                                                                                                                                                                                                      real_number(barrier, "barrier_level"), observation_mode,
                                                                                                                                                                                                                                                                      date_sequence(observation_dates, "observation_dates"), touch_state)); }, nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "observation_mode"_a = default_touch_observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = std::nullopt, asset_no_touch_doc.c_str());
    module.def("asset_no_touch_down", [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonChoice<ObservationMode> observation_mode, PythonDateSequence observation_dates, std::optional<PythonChoice<BarrierTouchState>> touch_state) { return unwrap(make_asset_no_touch_down(
                                                                                                                                                                                                                                                                        calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                                                                                                                                                                                                                                                                        real_number(barrier, "barrier_level"), observation_mode,
                                                                                                                                                                                                                                                                        date_sequence(observation_dates, "observation_dates"), touch_state)); }, nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "observation_mode"_a = default_touch_observation_mode, "observation_dates"_a = nb::make_tuple(), "touch_state"_a = std::nullopt, asset_no_touch_doc.c_str());

    auto accumulator = nb::class_<Accumulator>(
                           module, "Accumulator", R"doc(Immutable validated accumulator contract.)doc")
                           .def(nb::new_([](PythonReal strike, PythonReal knock_out,
                                            PythonReal daily_quantity, PythonReal acceleration,
                                            PythonReal accumulated_quantity, PythonDate effective_date,
                                            PythonDate expiry_date) {
                                    return unwrap(make_accumulator({real_number(strike, "strike"), real_number(knock_out, "knock_out_level"),
                                                                    real_number(daily_quantity, "daily_quantity"),
                                                                    real_number(acceleration, "acceleration_factor"),
                                                                    real_number(accumulated_quantity, "accumulated_quantity"),
                                                                    calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date")}));
                                }),
                                nb::kw_only(), "strike"_a, "knock_out_level"_a, "daily_quantity"_a,
                                "acceleration_factor"_a,
                                "accumulated_quantity"_a = AccumulatorTerms{}.accumulated_quantity, "effective_date"_a,
                                "expiry_date"_a, R"doc(Create a validated accumulator contract.

Parameters
----------
strike : float
    Positive purchase strike.
knock_out_level : float
    Positive upper knock-out level.
daily_quantity : float
    Non-negative base quantity accumulated per trading day.
acceleration_factor : float
    Non-negative quantity multiplier below the strike.
accumulated_quantity : float, optional
    Non-negative quantity already accumulated at valuation.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates, each anchored at 00:00 UTC.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the quantities, levels, or date ordering.)doc")
                           .def_prop_ro("strike", &Accumulator::strike, "Positive purchase strike.")
                           .def_prop_ro("knock_out_level", &Accumulator::knock_out_level,
                                        "Positive upper knock-out level.")
                           .def_prop_ro("daily_quantity", &Accumulator::daily_quantity,
                                        "Base quantity accumulated per trading day.")
                           .def_prop_ro("acceleration_factor", &Accumulator::acceleration_factor,
                                        "Quantity multiplier applied below the strike.")
                           .def_prop_ro("accumulated_quantity", &Accumulator::accumulated_quantity,
                                        "Quantity already accumulated at valuation.")
                           .def_prop_ro("effective_date", [](const Accumulator& value) { return python_date(value.effective_date()); }, "First date on which the contract is effective.")
                           .def_prop_ro("expiry_date", [](const Accumulator& value) { return python_date(value.expiry_date()); }, "Contract expiry at 00:00 UTC.");
    bind_value_equality(accumulator);
    bind_repr(accumulator, "Accumulator",
              {"strike", "knock_out_level", "daily_quantity", "acceleration_factor", "accumulated_quantity", "effective_date", "expiry_date"});

    bind_autocallable_instruments(module);
}

} // namespace kiyosi::python_binding
