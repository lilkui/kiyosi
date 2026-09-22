#include "_binding.hpp"

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

constexpr const char* cash_one_touch_doc = R"doc(Create a cash one-touch option.

The factory name selects an upper or lower barrier.

Parameters
----------
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
payout : float
    Fixed cash payout.
settlement_timing : SettlementTiming, optional
    Settle at the barrier hit or at expiry.
observation_mode : ObservationMode, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.

Returns
-------
TouchOption
    Validated immutable cash one-touch option.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the payoff, barrier, dates, or observation schedule.)doc";

constexpr const char* cash_no_touch_doc = R"doc(Create a cash no-touch option.

The factory name selects an upper or lower barrier. No-touch payoffs settle at
expiry.

Parameters
----------
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
payout : float
    Fixed cash payout.
observation_mode : ObservationMode, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.

Returns
-------
TouchOption
    Validated immutable cash no-touch option.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the payoff, barrier, dates, or observation schedule.)doc";

constexpr const char* asset_one_touch_doc = R"doc(Create an asset one-touch option.

The factory name selects an upper or lower barrier.

Parameters
----------
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
settlement_timing : SettlementTiming, optional
    Settle at the barrier hit or at expiry.
observation_mode : ObservationMode, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.

Returns
-------
TouchOption
    Validated immutable asset one-touch option.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the barrier, dates, or observation schedule.)doc";

constexpr const char* asset_no_touch_doc = R"doc(Create an asset no-touch option.

The factory name selects an upper or lower barrier. No-touch payoffs settle at
expiry.

Parameters
----------
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
observation_mode : ObservationMode, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.

Returns
-------
TouchOption
    Validated immutable asset no-touch option.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the barrier, dates, or observation schedule.)doc";

template <typename Instrument>
void bind_common_option_properties(nb::class_<Instrument>& binding, const char* name,
                                   std::initializer_list<ReprField> extra_fields = {})
{
    binding.def_prop_ro("option_type", &Instrument::option_type,
                        "Call or put payoff direction.")
        .def_prop_ro("strike", &Instrument::strike, "Positive strike price.")
        .def_prop_ro("effective_date", [](const Instrument& value) { return python_date(value.effective_date()); },
                     "First date on which the contract is effective.")
        .def_prop_ro("expiry_date", [](const Instrument& value) { return python_date(value.expiry_date()); },
                     "Contract expiry date.");
    bind_value_equality(binding);
    std::vector<ReprField> fields{{"option_type", "option_type"}, {"strike", "strike"}};
    fields.insert(fields.end(), extra_fields.begin(), extra_fields.end());
    fields.insert(fields.end(), {{"effective_date", "effective_date"}, {"expiry_date", "expiry_date"}});
    bind_repr(binding, name, fields);
}

template <typename AverageOptionType>
void bind_average_option(
    nb::module_& module, const char* name,
    Result<AverageOptionType> (*factory)(OptionType, double, Date, Date, Date, double))
{
    auto binding = nb::class_<AverageOptionType>(
        module, name, R"doc(Immutable validated average-price option.

Attributes
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
averaging_start_date : datetime.date
    First date included in the averaging period.
realized_average : float
    Average realized before the valuation date, or zero before averaging begins.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry date.)doc")
        .def(nb::new_([factory](OptionType type, PythonReal strike,
                                PythonDate averaging_start_date, PythonDate effective_date,
                                PythonDate expiry_date, PythonReal realized_average) {
                 return unwrap(factory(
                     type, real_number(strike, "strike"),
                     calendar_date(averaging_start_date, "averaging_start_date"),
                     calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                     real_number(realized_average, "realized_average")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "averaging_start_date"_a, "effective_date"_a,
             "expiry_date"_a, "realized_average"_a = default_realized_average,
             R"doc(Create a validated average-price option.

Parameters
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
averaging_start_date : datetime.date
    First date included in the averaging period.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry date.
realized_average : float, optional
    Average already realized; defaults to the core-owned pre-averaging value.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the terms or date ordering.)doc")
        .def_prop_ro("option_type", &AverageOptionType::option_type,
                     "Call or put payoff direction.")
        .def_prop_ro("strike", &AverageOptionType::strike, "Positive strike price.")
        .def_prop_ro("averaging_start_date", [](const AverageOptionType& value) {
            return python_date(value.averaging_start_date());
        }, "First date included in the averaging period.")
        .def_prop_ro("realized_average", &AverageOptionType::realized_average,
                     "Average realized before valuation.")
        .def_prop_ro("effective_date", [](const AverageOptionType& value) { return python_date(value.effective_date()); },
                     "First date on which the contract is effective.")
        .def_prop_ro("expiry_date", [](const AverageOptionType& value) { return python_date(value.expiry_date()); },
                     "Contract expiry date.");
    bind_value_equality(binding);
    bind_repr(binding, name,
              {{"option_type", "option_type"}, {"strike", "strike"},
               {"averaging_start_date", "averaging_start_date"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"},
               {"realized_average", "realized_average"}});
}

} // namespace

void bind_instruments(nb::module_& module)
{
    auto european = nb::class_<EuropeanOption>(
        module, "EuropeanOption", R"doc(Immutable validated European vanilla option.

The payoff can be exercised only at expiry.

Attributes
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry date.)doc")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonDate effective_date,
                        PythonDate expiry_date) {
                 return unwrap(make_european_option(
                     type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                     calendar_date(expiry_date, "expiry_date")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
             R"doc(Create a validated European option.

Parameters
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry date.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the strike or date ordering.)doc");
    bind_common_option_properties(european, "EuropeanOption");

    auto american = nb::class_<AmericanOption>(
        module, "AmericanOption", R"doc(Immutable validated American vanilla option.

The payoff may be exercised from the effective date through expiry.

Attributes
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date : datetime.date
    First exercise date.
expiry_date : datetime.date
    Last exercise date.)doc")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonDate effective_date,
                        PythonDate expiry_date) {
                 return unwrap(make_american_option(
                     type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                     calendar_date(expiry_date, "expiry_date")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
             R"doc(Create a validated American option.

Parameters
----------
option_type : OptionType
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
    bind_common_option_properties(american, "AmericanOption");

    auto cash = nb::class_<CashOrNothingOption>(
        module, "CashOrNothingOption", R"doc(Immutable validated cash-or-nothing digital option.

Attributes
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
payout : float
    Fixed cash amount paid when the option finishes in the money.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry date.)doc")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonReal payout,
                        PythonDate effective_date, PythonDate expiry_date) {
                 return unwrap(make_cash_or_nothing_option(
                     type, real_number(strike, "strike"), real_number(payout, "payout"),
                     calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "payout"_a, "effective_date"_a,
             "expiry_date"_a, R"doc(Create a validated cash-or-nothing option.

Parameters
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
payout : float
    Fixed cash amount paid when the option finishes in the money.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry date.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the payoff terms or date ordering.)doc")
        .def_prop_ro("payout", &CashOrNothingOption::payout,
                     "Fixed in-the-money cash payout.");
    bind_common_option_properties(cash, "CashOrNothingOption", {{"payout", "payout"}});

    auto asset = nb::class_<AssetOrNothingOption>(
        module, "AssetOrNothingOption", R"doc(Immutable validated asset-or-nothing digital option.

Attributes
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry date.)doc")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonDate effective_date,
                        PythonDate expiry_date) {
                 return unwrap(make_asset_or_nothing_option(
                     type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                     calendar_date(expiry_date, "expiry_date")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
             R"doc(Create a validated asset-or-nothing option.

Parameters
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date : datetime.date
    First date on which the contract is effective.
expiry_date : datetime.date
    Contract expiry date.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the strike or date ordering.)doc");
    bind_common_option_properties(asset, "AssetOrNothingOption");

    bind_average_option(module, "GeometricAveragePriceOption", &make_geometric_average_option);
    bind_average_option(module, "ArithmeticAveragePriceOption", &make_arithmetic_average_option);

    auto barrier = nb::class_<BarrierOption>(
        module, "BarrierOption", R"doc(Immutable validated barrier option.

Scheduled barriers are observed only on ``observation_dates``; continuous
barriers require no schedule.

Attributes
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
barrier_type : BarrierType
    Barrier direction and knock-in or knock-out behavior.
rebate : float
    Rebate amount.
rebate_timing : RebateTiming
    Time at which the rebate is paid.
observation_mode : ObservationMode
    Continuous or scheduled monitoring.
observation_dates : list[datetime.date]
    Ordered scheduled monitoring dates.)doc")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonDate effective_date,
                        PythonDate expiry_date, PythonReal barrier, BarrierType barrier_type,
                        PythonReal rebate, RebateTiming rebate_timing,
                        ObservationMode observation_mode, PythonDateSequence observation_dates) {
                 return unwrap(make_barrier_option({
                     type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                     calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                     barrier_type, real_number(rebate, "rebate"), rebate_timing, observation_mode,
                     date_sequence(observation_dates, "observation_dates")}));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
             "barrier_level"_a, "barrier_type"_a, "rebate"_a = BarrierOptionTerms{}.rebate,
             "rebate_timing"_a = BarrierOptionTerms{}.rebate_timing,
             "observation_mode"_a = BarrierOptionTerms{}.observation_mode,
             "observation_dates"_a = nb::make_tuple(),
             R"doc(Create a validated barrier option.

Parameters
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
barrier_type : BarrierType
    Barrier direction and knock-in or knock-out behavior.
rebate : float, optional
    Rebate amount. Uses the core default when omitted.
rebate_timing : RebateTiming, optional
    Time at which the rebate is paid.
observation_mode : ObservationMode, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring; omitted for continuous monitoring.

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the payoff, barrier, dates, or observation schedule.)doc")
        .def_prop_ro("option_type", &BarrierOption::option_type,
                     "Call or put payoff direction.")
        .def_prop_ro("strike", &BarrierOption::strike, "Positive strike price.")
        .def_prop_ro("effective_date", [](const BarrierOption& value) { return python_date(value.effective_date()); },
                     "First date on which the contract is effective.")
        .def_prop_ro("expiry_date", [](const BarrierOption& value) { return python_date(value.expiry_date()); },
                     "Contract expiry date.")
        .def_prop_ro("barrier_level", &BarrierOption::barrier_level,
                     "Positive barrier level.")
        .def_prop_ro("barrier_type", &BarrierOption::barrier_type,
                     "Barrier direction and activation behavior.")
        .def_prop_ro("rebate", &BarrierOption::rebate, "Barrier rebate amount.")
        .def_prop_ro("rebate_timing", &BarrierOption::rebate_timing,
                     "Time at which the rebate is paid.")
        .def_prop_ro("observation_mode", &BarrierOption::observation_mode,
                     "Continuous or scheduled monitoring mode.")
        .def_prop_ro("observation_dates", [](const BarrierOption& value) {
            PythonDateList output;
            for (const Date item : value.observation_dates()) output.append(python_date(item));
            return output;
        }, "Copy of the ordered scheduled observation dates.");
    bind_value_equality(barrier);
    bind_repr(barrier, "BarrierOption",
              {{"option_type", "option_type"}, {"strike", "strike"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"},
               {"barrier_level", "barrier_level"}, {"barrier_type", "barrier_type"},
               {"rebate", "rebate"}, {"rebate_timing", "rebate_timing"},
               {"observation_mode", "observation_mode"},
               {"observation_dates", "observation_dates"}});

    auto binary_barrier = nb::class_<BinaryBarrierOption>(
        module, "BinaryBarrierOption", R"doc(Immutable validated strike-based binary barrier option.

Instances are created by :func:`cash_binary_barrier_option` or
:func:`asset_binary_barrier_option`.

Attributes
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
barrier_type : BarrierType
    Barrier direction and activation behavior.
payoff_type : PayoffType
    Cash or asset delivery.
payout : float or None
    Cash payout, or ``None`` for an asset payoff.
observation_mode : ObservationMode
    Continuous or scheduled monitoring.
observation_dates : list[datetime.date]
    Ordered scheduled monitoring dates.)doc")
        .def_prop_ro("option_type", &BinaryBarrierOption::option_type,
                     "Call or put payoff direction.")
        .def_prop_ro("strike", &BinaryBarrierOption::strike, "Positive strike price.")
        .def_prop_ro("effective_date", [](const BinaryBarrierOption& value) { return python_date(value.effective_date()); },
                     "First date on which the contract is effective.")
        .def_prop_ro("expiry_date", [](const BinaryBarrierOption& value) { return python_date(value.expiry_date()); },
                     "Contract expiry date.")
        .def_prop_ro("barrier_level", &BinaryBarrierOption::barrier_level,
                     "Positive barrier level.")
        .def_prop_ro("barrier_type", &BinaryBarrierOption::barrier_type,
                     "Barrier direction and activation behavior.")
        .def_prop_ro("payoff_type", &BinaryBarrierOption::payoff_type,
                     "Cash or asset delivery form.")
        .def_prop_ro("payout", [](const BinaryBarrierOption& value) -> std::optional<double> {
            if (const auto* cash = std::get_if<CashOrNothingPayoff>(&value.payoff()))
                return cash->payout();
            return std::nullopt;
        }, "Cash payout, or None for an asset payoff.")
        .def_prop_ro("observation_mode", &BinaryBarrierOption::observation_mode,
                     "Continuous or scheduled monitoring mode.")
        .def_prop_ro("observation_dates", [](const BinaryBarrierOption& value) {
            PythonDateList output;
            for (const Date item : value.observation_dates()) output.append(python_date(item));
            return output;
        }, "Copy of the ordered scheduled observation dates.");
    bind_value_equality(binary_barrier);
    bind_repr(binary_barrier, "BinaryBarrierOption",
               {{"option_type", "option_type"}, {"strike", "strike"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"},
               {"barrier_level", "barrier_level"}, {"barrier_type", "barrier_type"},
               {"payoff_type", "payoff_type"}, {"payout", "payout"},
               {"observation_mode", "observation_mode"},
               {"observation_dates", "observation_dates"}});

    module.def("cash_binary_barrier_option",
               [](OptionType type, PythonReal strike, PythonDate effective_date,
                  PythonDate expiry_date, PythonReal barrier, BarrierType barrier_type,
                  PythonReal payout, ObservationMode observation_mode,
                  PythonDateSequence observation_dates) {
                   return unwrap(make_cash_binary_barrier_option(
                       {type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                        calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                        barrier_type, observation_mode, date_sequence(observation_dates, "observation_dates")},
                       real_number(payout, "payout")));
               },
               nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
               "barrier_level"_a, "barrier_type"_a, "payout"_a,
               "observation_mode"_a = BinaryBarrierTerms{}.observation_mode,
               "observation_dates"_a = nb::make_tuple(),
               R"doc(Create a cash-or-nothing binary barrier option.

Parameters
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
barrier_type : BarrierType
    Barrier direction and activation behavior.
payout : float
    Fixed cash amount paid when the payoff and barrier conditions hold.
observation_mode : ObservationMode, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.

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
    module.def("asset_binary_barrier_option",
               [](OptionType type, PythonReal strike, PythonDate effective_date,
                  PythonDate expiry_date, PythonReal barrier, BarrierType barrier_type,
                  ObservationMode observation_mode, PythonDateSequence observation_dates) {
                   return unwrap(make_asset_binary_barrier_option(
                       {type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                        calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                        barrier_type, observation_mode, date_sequence(observation_dates, "observation_dates")}));
               },
               nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
               "barrier_level"_a, "barrier_type"_a,
               "observation_mode"_a = BinaryBarrierTerms{}.observation_mode,
               "observation_dates"_a = nb::make_tuple(),
               R"doc(Create an asset-or-nothing binary barrier option.

Parameters
----------
option_type : OptionType
    Call or put payoff direction.
strike : float
    Positive strike price.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
barrier_type : BarrierType
    Barrier direction and activation behavior.
observation_mode : ObservationMode, optional
    Continuous or scheduled monitoring.
observation_dates : iterable[datetime.date], optional
    Required schedule for scheduled monitoring.

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
factory functions.

Attributes
----------
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.
barrier_level : float
    Positive barrier level.
is_one_touch : bool
    Whether hitting the barrier activates rather than cancels the payoff.
is_up : bool
    Whether the barrier is above the spot direction.
payoff_type : PayoffType
    Cash or asset delivery.
payout : float or None
    Cash payout, or ``None`` for an asset payoff.
settlement_timing : SettlementTiming
    Settlement time for a one-touch payoff.
observation_mode : ObservationMode
    Continuous or scheduled monitoring.
observation_dates : list[datetime.date]
    Ordered scheduled monitoring dates.)doc")
        .def_prop_ro("effective_date", [](const TouchOption& value) { return python_date(value.effective_date()); },
                     "First date on which the contract is effective.")
        .def_prop_ro("expiry_date", [](const TouchOption& value) { return python_date(value.expiry_date()); },
                     "Contract expiry date.")
        .def_prop_ro("barrier_level", &TouchOption::barrier_level,
                     "Positive barrier level.")
        .def_prop_ro("is_one_touch", &TouchOption::is_one_touch,
                     "Whether hitting the barrier activates the payoff.")
        .def_prop_ro("is_up", &TouchOption::is_up,
                     "Whether the contract uses an upper barrier.")
        .def_prop_ro("payoff_type", &TouchOption::payoff_type,
                     "Cash or asset delivery form.")
        .def_prop_ro("payout", [](const TouchOption& value) -> std::optional<double> {
            if (const auto* cash = std::get_if<CashOrNothingPayoff>(&value.payoff()))
                return cash->payout();
            return std::nullopt;
        }, "Cash payout, or None for an asset payoff.")
        .def_prop_ro("settlement_timing", &TouchOption::settlement_timing,
                     "Settlement time for a one-touch payoff.")
        .def_prop_ro("observation_mode", &TouchOption::observation_mode,
                     "Continuous or scheduled monitoring mode.")
        .def_prop_ro("observation_dates", [](const TouchOption& value) {
            PythonDateList output;
            for (const Date item : value.observation_dates()) output.append(python_date(item));
            return output;
        }, "Copy of the ordered scheduled observation dates.");
    bind_value_equality(touch);
    bind_repr(touch, "TouchOption",
              {{"effective_date", "effective_date"}, {"expiry_date", "expiry_date"},
               {"barrier_level", "barrier_level"}, {"is_one_touch", "is_one_touch"},
               {"is_up", "is_up"}, {"payoff_type", "payoff_type"},
               {"payout", "payout"}, {"settlement_timing", "settlement_timing"},
               {"observation_mode", "observation_mode"},
               {"observation_dates", "observation_dates"}});

    module.def("cash_one_touch_up",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  PythonReal payout, SettlementTiming settlement_timing,
                  ObservationMode observation_mode, PythonDateSequence observation_dates) {
                   return unwrap(make_cash_one_touch_up(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                       settlement_timing,
                       observation_mode, date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a,
               "settlement_timing"_a = SettlementTiming::at_expiry,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               cash_one_touch_doc);
    module.def("cash_one_touch_down",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  PythonReal payout, SettlementTiming settlement_timing,
                  ObservationMode observation_mode, PythonDateSequence observation_dates) {
                   return unwrap(make_cash_one_touch_down(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                       settlement_timing,
                       observation_mode, date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a,
               "settlement_timing"_a = SettlementTiming::at_expiry,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               cash_one_touch_doc);
    module.def("cash_no_touch_up",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonReal payout,
                  ObservationMode observation_mode, PythonDateSequence observation_dates) {
                   return unwrap(make_cash_no_touch_up(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                       observation_mode, date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               cash_no_touch_doc);
    module.def("cash_no_touch_down",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonReal payout,
                  ObservationMode observation_mode, PythonDateSequence observation_dates) {
                   return unwrap(make_cash_no_touch_down(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                       observation_mode, date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               cash_no_touch_doc);
    module.def("asset_one_touch_up",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  SettlementTiming settlement_timing, ObservationMode observation_mode,
                  PythonDateSequence observation_dates) {
                   return unwrap(make_asset_one_touch_up(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), settlement_timing, observation_mode,
                       date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a,
               "settlement_timing"_a = SettlementTiming::at_expiry,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               asset_one_touch_doc);
    module.def("asset_one_touch_down",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  SettlementTiming settlement_timing, ObservationMode observation_mode,
                  PythonDateSequence observation_dates) {
                   return unwrap(make_asset_one_touch_down(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), settlement_timing, observation_mode,
                       date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a,
               "settlement_timing"_a = SettlementTiming::at_expiry,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               asset_one_touch_doc);
    module.def("asset_no_touch_up",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  ObservationMode observation_mode, PythonDateSequence observation_dates) {
                   return unwrap(make_asset_no_touch_up(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), observation_mode,
                       date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               asset_no_touch_doc);
    module.def("asset_no_touch_down",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  ObservationMode observation_mode, PythonDateSequence observation_dates) {
                   return unwrap(make_asset_no_touch_down(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), observation_mode,
                       date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               asset_no_touch_doc);

    auto accumulator = nb::class_<Accumulator>(
        module, "Accumulator", R"doc(Immutable validated accumulator contract.

Attributes
----------
strike : float
    Positive purchase strike.
knock_out_level : float
    Positive upper knock-out level.
daily_quantity : float
    Base quantity accumulated per trading day.
acceleration_factor : float
    Quantity multiplier applied below the strike.
accumulated_quantity : float
    Quantity already accumulated at valuation.
effective_date, expiry_date : datetime.date
    Contract effective and expiry dates.)doc")
        .def(nb::new_([](PythonReal strike, PythonReal knock_out,
                        PythonReal daily_quantity, PythonReal acceleration,
                        PythonReal accumulated_quantity, PythonDate effective_date,
                        PythonDate expiry_date) {
                 return unwrap(make_accumulator({
                     real_number(strike, "strike"), real_number(knock_out, "knock_out_level"),
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
    Contract effective and expiry dates.

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
        .def_prop_ro("effective_date", [](const Accumulator& value) { return python_date(value.effective_date()); },
                     "First date on which the contract is effective.")
        .def_prop_ro("expiry_date", [](const Accumulator& value) { return python_date(value.expiry_date()); },
                     "Contract expiry date.");
    bind_value_equality(accumulator);
    bind_repr(accumulator, "Accumulator",
              {{"strike", "strike"}, {"knock_out_level", "knock_out_level"},
               {"daily_quantity", "daily_quantity"},
               {"acceleration_factor", "acceleration_factor"},
               {"accumulated_quantity", "accumulated_quantity"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"}});

    bind_structured_instruments(module);
}

} // namespace kiyosi::python_binding
