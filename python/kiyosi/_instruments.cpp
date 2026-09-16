#include "_binding.hpp"

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

template <typename Instrument>
void bind_common_option_properties(nb::class_<Instrument>& binding, const char* name,
                                   std::initializer_list<ReprField> extra_fields = {})
{
    binding.def_prop_ro("type", &Instrument::type)
        .def_prop_ro("strike", &Instrument::strike)
        .def_prop_ro("effective", [](const Instrument& value) { return python_date(value.effective()); })
        .def_prop_ro("expiry", [](const Instrument& value) { return python_date(value.expiry()); });
    bind_value_equality(binding);
    std::vector<ReprField> fields{{"type", "type"}, {"strike", "strike"}};
    fields.insert(fields.end(), extra_fields.begin(), extra_fields.end());
    fields.insert(fields.end(), {{"effective", "effective"}, {"expiry", "expiry"}});
    bind_repr(binding, name, fields);
}

template <typename AverageOptionType>
void bind_average_option(
    nb::module_& module, const char* name,
    result<AverageOptionType> (*factory)(option_type, double, date, date, date, double))
{
    auto binding = nb::class_<AverageOptionType>(
        module, name, "Immutable validated average-price option.")
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
             "expiry"_a, "realized_average"_a = default_realized_average,
             "Create a validated average-price option.")
        .def_prop_ro("type", &AverageOptionType::type)
        .def_prop_ro("strike", &AverageOptionType::strike)
        .def_prop_ro("average_start", [](const AverageOptionType& value) {
            return python_date(value.average_start());
        })
        .def_prop_ro("realized_average", &AverageOptionType::realized_average)
        .def_prop_ro("effective", [](const AverageOptionType& value) { return python_date(value.effective()); })
        .def_prop_ro("expiry", [](const AverageOptionType& value) { return python_date(value.expiry()); });
    bind_value_equality(binding);
    bind_repr(binding, name,
              {{"type", "type"}, {"strike", "strike"},
               {"average_start", "average_start"},
               {"effective", "effective"}, {"expiry", "expiry"},
               {"realized_average", "realized_average"}});
}

} // namespace

void bind_instruments(nb::module_& module)
{
    auto european = nb::class_<EuropeanOption>(
        module, "EuropeanOption", "Immutable validated European vanilla option.")
        .def(nb::new_([](option_type type, PythonReal strike, PythonDate effective,
                        PythonDate expiry) {
                 return unwrap(make_european_option(
                     type, real_number(strike, "strike"), calendar_date(effective, "effective"),
                     calendar_date(expiry, "expiry")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "effective"_a, "expiry"_a,
             "Create a validated European option.");
    bind_common_option_properties(european, "EuropeanOption");

    auto american = nb::class_<AmericanOption>(
        module, "AmericanOption", "Immutable validated American vanilla option.")
        .def(nb::new_([](option_type type, PythonReal strike, PythonDate effective,
                        PythonDate expiry) {
                 return unwrap(make_american_option(
                     type, real_number(strike, "strike"), calendar_date(effective, "effective"),
                     calendar_date(expiry, "expiry")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "effective"_a, "expiry"_a,
             "Create a validated American option.");
    bind_common_option_properties(american, "AmericanOption");

    auto cash = nb::class_<EuropeanCashOrNothingOption>(
        module, "CashOrNothingOption", "Immutable validated cash-or-nothing option.")
        .def(nb::new_([](option_type type, PythonReal strike, PythonReal payout,
                        PythonDate effective, PythonDate expiry) {
                 return unwrap(make_cash_or_nothing_option(
                     type, real_number(strike, "strike"), real_number(payout, "payout"),
                     calendar_date(effective, "effective"), calendar_date(expiry, "expiry")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "payout"_a, "effective"_a,
             "expiry"_a, "Create a validated cash-or-nothing option.")
        .def_prop_ro("payout", &EuropeanCashOrNothingOption::payout);
    bind_common_option_properties(cash, "CashOrNothingOption", {{"payout", "payout"}});

    auto asset = nb::class_<EuropeanAssetOrNothingOption>(
        module, "AssetOrNothingOption", "Immutable validated asset-or-nothing option.")
        .def(nb::new_([](option_type type, PythonReal strike, PythonDate effective,
                        PythonDate expiry) {
                 return unwrap(make_asset_or_nothing_option(
                     type, real_number(strike, "strike"), calendar_date(effective, "effective"),
                     calendar_date(expiry, "expiry")));
             }),
             nb::kw_only(), "type"_a, "strike"_a, "effective"_a, "expiry"_a,
             "Create a validated asset-or-nothing option.");
    bind_common_option_properties(asset, "AssetOrNothingOption");

    bind_average_option(module, "GeometricAverageOption", &make_geometric_average_option);
    bind_average_option(module, "ArithmeticAverageOption", &make_arithmetic_average_option);

    auto barrier = nb::class_<BarrierOption>(
        module, "BarrierOption", "Immutable validated barrier option.")
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
             "barrier"_a, "barrier_kind"_a, "rebate"_a = BarrierOptionTerms{}.rebate,
             "rebate_payment"_a = BarrierOptionTerms{}.rebate_payment,
             "observation"_a = BarrierOptionTerms{}.observation,
             "observations"_a = nb::make_tuple(),
             "Create a validated barrier option.")
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
    bind_value_equality(barrier);
    bind_repr(barrier, "BarrierOption",
              {{"type", "type"}, {"strike", "strike"},
               {"effective", "effective"}, {"expiry", "expiry"},
               {"barrier", "barrier"}, {"barrier_kind", "barrier_kind"},
               {"rebate", "rebate"}, {"rebate_payment", "rebate_payment"},
               {"observation", "observation"},
               {"observations", "observation_dates"}});

    auto binary_barrier = nb::class_<BinaryBarrierOption>(
        module, "BinaryBarrierOption", "Immutable validated binary barrier option.")
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
             "asset_settlement"_a = BinaryBarrierTerms{}.asset_settlement,
             "settlement_timing"_a = BinaryBarrierTerms{}.settlement_timing,
             "observation"_a = BinaryBarrierTerms{}.observation,
             "observations"_a = nb::make_tuple(),
             "Create a validated binary barrier option.")
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
    bind_value_equality(binary_barrier);
    bind_repr(binary_barrier, "BinaryBarrierOption",
              {{"type", "type"}, {"strike", "strike"},
               {"effective", "effective"}, {"expiry", "expiry"},
               {"barrier", "barrier"}, {"barrier_kind", "barrier_kind"},
               {"payout", "payout"}, {"asset_settlement", "asset_settlement"},
               {"settlement_timing", "settlement_timing"},
               {"observation", "observation"},
               {"observations", "observation_dates"}});

    auto accumulator = nb::class_<Accumulator>(
        module, "Accumulator", "Immutable validated accumulator contract.")
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
             "acceleration"_a,
             "accumulated_quantity"_a = AccumulatorTerms{}.accumulated_quantity, "effective"_a,
             "expiry"_a, "Create a validated accumulator contract.")
        .def_prop_ro("strike", &Accumulator::strike)
        .def_prop_ro("knock_out", &Accumulator::knock_out)
        .def_prop_ro("daily_quantity", &Accumulator::daily_quantity)
        .def_prop_ro("acceleration", &Accumulator::acceleration)
        .def_prop_ro("accumulated_quantity", &Accumulator::accumulated_quantity)
        .def_prop_ro("effective", [](const Accumulator& value) { return python_date(value.effective()); })
        .def_prop_ro("expiry", [](const Accumulator& value) { return python_date(value.expiry()); });
    bind_value_equality(accumulator);
    bind_repr(accumulator, "Accumulator",
              {{"strike", "strike"}, {"knock_out", "knock_out"},
               {"daily_quantity", "daily_quantity"},
               {"acceleration", "acceleration"},
               {"accumulated_quantity", "accumulated_quantity"},
               {"effective", "effective"}, {"expiry", "expiry"}});

    bind_structured_instruments(module);
}

} // namespace kiyosi::python_binding
