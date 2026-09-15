#include "_binding.hpp"

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

template <typename Instrument>
void bind_common_option_properties(nb::class_<Instrument>& binding)
{
    binding.def_prop_ro("type", &Instrument::type)
        .def_prop_ro("strike", &Instrument::strike)
        .def_prop_ro("effective", [](const Instrument& value) { return python_date(value.effective()); })
        .def_prop_ro("expiry", [](const Instrument& value) { return python_date(value.expiry()); });
}

template <typename AverageOptionType>
void bind_average_option(
    nb::module_& module, const char* name,
    result<AverageOptionType> (*factory)(option_type, double, date, date, date, double))
{
    nb::class_<AverageOptionType>(module, name)
        .def(nb::new_([factory](option_type type, PythonReal strike,
                                PythonDate average_start, PythonDate effective,
                                PythonDate expiry, PythonReal realized_average) {
                 return unwrap(factory(
                     type, real_number(strike, "strike"),
                     calendar_date(average_start, "average_start"),
                     calendar_date(effective, "effective"), calendar_date(expiry, "expiry"),
                     real_number(realized_average, "realized_average")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "average_start"_a, "effective"_a,
             "expiry"_a, "realized_average"_a = 0.0)
        .def_prop_ro("type", &AverageOptionType::type)
        .def_prop_ro("strike", &AverageOptionType::strike)
        .def_prop_ro("average_start", [](const AverageOptionType& value) {
            return python_date(value.average_start());
        })
        .def_prop_ro("realized_average", &AverageOptionType::realized_average)
        .def_prop_ro("effective", [](const AverageOptionType& value) { return python_date(value.effective()); })
        .def_prop_ro("expiry", [](const AverageOptionType& value) { return python_date(value.expiry()); });
}

} // namespace

void bind_instruments(nb::module_& module)
{
    auto european = nb::class_<EuropeanOption>(module, "EuropeanOption")
        .def(nb::new_([](option_type type, PythonReal strike, PythonDate effective,
                        PythonDate expiry) {
                 return unwrap(make_european_option(
                     type, real_number(strike, "strike"), calendar_date(effective, "effective"),
                     calendar_date(expiry, "expiry")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "effective"_a, "expiry"_a);
    bind_common_option_properties(european);

    auto american = nb::class_<AmericanOption>(module, "AmericanOption")
        .def(nb::new_([](option_type type, PythonReal strike, PythonDate effective,
                        PythonDate expiry) {
                 return unwrap(make_american_option(
                     type, real_number(strike, "strike"), calendar_date(effective, "effective"),
                     calendar_date(expiry, "expiry")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "effective"_a, "expiry"_a);
    bind_common_option_properties(american);

    auto cash = nb::class_<EuropeanCashOrNothingOption>(module, "CashOrNothingOption")
        .def(nb::new_([](option_type type, PythonReal strike, PythonReal payout,
                        PythonDate effective, PythonDate expiry) {
                 return unwrap(make_cash_or_nothing_option(
                     type, real_number(strike, "strike"), real_number(payout, "payout"),
                     calendar_date(effective, "effective"), calendar_date(expiry, "expiry")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "payout"_a, "effective"_a,
             "expiry"_a)
        .def_prop_ro("payout", &EuropeanCashOrNothingOption::payout);
    bind_common_option_properties(cash);

    auto asset = nb::class_<EuropeanAssetOrNothingOption>(module, "AssetOrNothingOption")
        .def(nb::new_([](option_type type, PythonReal strike, PythonDate effective,
                        PythonDate expiry) {
                 return unwrap(make_asset_or_nothing_option(
                     type, real_number(strike, "strike"), calendar_date(effective, "effective"),
                     calendar_date(expiry, "expiry")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "effective"_a, "expiry"_a);
    bind_common_option_properties(asset);

    bind_average_option(module, "GeometricAverageOption", &make_geometric_average_option);
    bind_average_option(module, "ArithmeticAverageOption", &make_arithmetic_average_option);

    nb::class_<BarrierOption>(module, "BarrierOption")
        .def(nb::new_([](option_type type, PythonReal strike, PythonDate effective,
                        PythonDate expiry, PythonReal barrier, barrier_type barrier_kind,
                        PythonReal rebate, rebate_timing rebate_payment,
                        observation_mode observation, PythonDateSequence observations) {
                 return unwrap(make_barrier_option({
                     type, real_number(strike, "strike"), calendar_date(effective, "effective"),
                     calendar_date(expiry, "expiry"), real_number(barrier, "barrier"),
                     barrier_kind, real_number(rebate, "rebate"), rebate_payment, observation,
                     date_sequence(observations, "observations")}));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "effective"_a, "expiry"_a,
             "barrier"_a, "barrier_kind"_a, "rebate"_a = 0.0,
             "rebate_payment"_a = rebate_timing::at_expiry,
             "observation"_a = observation_mode::continuous,
             "observations"_a = nb::make_tuple())
        .def_prop_ro("type", &BarrierOption::type)
        .def_prop_ro("strike", &BarrierOption::strike)
        .def_prop_ro("effective", [](const BarrierOption& value) { return python_date(value.effective()); })
        .def_prop_ro("expiry", [](const BarrierOption& value) { return python_date(value.expiry()); })
        .def_prop_ro("barrier", &BarrierOption::barrier)
        .def_prop_ro("barrier_kind", &BarrierOption::barrier_kind)
        .def_prop_ro("rebate", &BarrierOption::rebate)
        .def_prop_ro("rebate_payment", &BarrierOption::rebate_payment)
        .def_prop_ro("observation", &BarrierOption::observation)
        .def_prop_ro("observation_dates", [](const BarrierOption& value) {
            PythonDateList output;
            for (const date item : value.observation_dates()) output.append(python_date(item));
            return output;
        });

    nb::class_<BinaryBarrierOption>(module, "BinaryBarrierOption")
        .def(nb::new_([](PythonOptionType type, PythonReal strike, PythonDate effective,
                        PythonDate expiry, PythonReal barrier, barrier_type barrier_kind,
                        PythonReal payout, bool asset_settlement,
                        rebate_timing settlement_timing, observation_mode observation,
                        PythonDateSequence observations) {
                 std::optional<option_type> option;
                 if (!type.is_none()) option = nb::cast<option_type>(type);
                 return unwrap(make_binary_barrier_option({
                     option, real_number(strike, "strike"), calendar_date(effective, "effective"),
                     calendar_date(expiry, "expiry"), real_number(barrier, "barrier"),
                     barrier_kind, real_number(payout, "payout"), asset_settlement,
                     settlement_timing, observation, date_sequence(observations, "observations")}));
             }),
             nb::kw_only(), "type"_a = nb::none(), "strike"_a, "effective"_a,
             "expiry"_a, "barrier"_a, "barrier_kind"_a, "payout"_a,
             "asset_settlement"_a = false,
             "settlement_timing"_a = rebate_timing::at_expiry,
             "observation"_a = observation_mode::continuous,
             "observations"_a = nb::make_tuple())
        .def_prop_ro("type", &BinaryBarrierOption::type)
        .def_prop_ro("strike", &BinaryBarrierOption::strike)
        .def_prop_ro("effective", [](const BinaryBarrierOption& value) { return python_date(value.effective()); })
        .def_prop_ro("expiry", [](const BinaryBarrierOption& value) { return python_date(value.expiry()); })
        .def_prop_ro("barrier", &BinaryBarrierOption::barrier)
        .def_prop_ro("barrier_kind", &BinaryBarrierOption::barrier_kind)
        .def_prop_ro("payout", &BinaryBarrierOption::payout)
        .def_prop_ro("asset_settlement", &BinaryBarrierOption::asset_settlement)
        .def_prop_ro("settlement_timing", &BinaryBarrierOption::settlement_timing)
        .def_prop_ro("observation", &BinaryBarrierOption::observation)
        .def_prop_ro("observation_dates", [](const BinaryBarrierOption& value) {
            PythonDateList output;
            for (const date item : value.observation_dates()) output.append(python_date(item));
            return output;
        });

    nb::class_<Accumulator>(module, "Accumulator")
        .def(nb::new_([](PythonReal strike, PythonReal knock_out,
                        PythonReal daily_quantity, PythonReal acceleration,
                        PythonReal accumulated_quantity, PythonDate effective,
                        PythonDate expiry) {
                 return unwrap(make_accumulator({
                     real_number(strike, "strike"), real_number(knock_out, "knock_out"),
                     real_number(daily_quantity, "daily_quantity"),
                     real_number(acceleration, "acceleration"),
                     real_number(accumulated_quantity, "accumulated_quantity"),
                     calendar_date(effective, "effective"), calendar_date(expiry, "expiry")}));
             }),
             nb::kw_only(), "strike"_a, "knock_out"_a, "daily_quantity"_a,
             "acceleration"_a, "accumulated_quantity"_a = 0.0, "effective"_a,
             "expiry"_a)
        .def_prop_ro("strike", &Accumulator::strike)
        .def_prop_ro("knock_out", &Accumulator::knock_out)
        .def_prop_ro("daily_quantity", &Accumulator::daily_quantity)
        .def_prop_ro("acceleration", &Accumulator::acceleration)
        .def_prop_ro("accumulated_quantity", &Accumulator::accumulated_quantity)
        .def_prop_ro("effective", [](const Accumulator& value) { return python_date(value.effective()); })
        .def_prop_ro("expiry", [](const Accumulator& value) { return python_date(value.expiry()); });

    bind_structured_instruments(module);
}

} // namespace kiyosi::python_binding
