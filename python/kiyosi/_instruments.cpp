#include "_binding.hpp"

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

template <typename Instrument>
void bind_common_option_properties(nb::class_<Instrument>& binding, const char* name,
                                   std::initializer_list<ReprField> extra_fields = {})
{
    binding.def_prop_ro("option_type", &Instrument::option_type)
        .def_prop_ro("strike", &Instrument::strike)
        .def_prop_ro("effective_date", [](const Instrument& value) { return python_date(value.effective_date()); })
        .def_prop_ro("expiry_date", [](const Instrument& value) { return python_date(value.expiry_date()); });
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
        module, name, "Immutable validated average-price option.")
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
             "Create a validated average-price option.")
        .def_prop_ro("option_type", &AverageOptionType::option_type)
        .def_prop_ro("strike", &AverageOptionType::strike)
        .def_prop_ro("averaging_start_date", [](const AverageOptionType& value) {
            return python_date(value.averaging_start_date());
        })
        .def_prop_ro("realized_average", &AverageOptionType::realized_average)
        .def_prop_ro("effective_date", [](const AverageOptionType& value) { return python_date(value.effective_date()); })
        .def_prop_ro("expiry_date", [](const AverageOptionType& value) { return python_date(value.expiry_date()); });
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
        module, "EuropeanOption", "Immutable validated European vanilla option.")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonDate effective_date,
                        PythonDate expiry_date) {
                 return unwrap(make_european_option(
                     type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                     calendar_date(expiry_date, "expiry_date")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
             "Create a validated European option.");
    bind_common_option_properties(european, "EuropeanOption");

    auto american = nb::class_<AmericanOption>(
        module, "AmericanOption", "Immutable validated American vanilla option.")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonDate effective_date,
                        PythonDate expiry_date) {
                 return unwrap(make_american_option(
                     type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                     calendar_date(expiry_date, "expiry_date")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
             "Create a validated American option.");
    bind_common_option_properties(american, "AmericanOption");

    auto cash = nb::class_<CashOrNothingOption>(
        module, "CashOrNothingOption", "Immutable validated cash-or-nothing option.")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonReal payout,
                        PythonDate effective_date, PythonDate expiry_date) {
                 return unwrap(make_cash_or_nothing_option(
                     type, real_number(strike, "strike"), real_number(payout, "payout"),
                     calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "payout"_a, "effective_date"_a,
             "expiry_date"_a, "Create a validated cash-or-nothing option.")
        .def_prop_ro("payout", &CashOrNothingOption::payout);
    bind_common_option_properties(cash, "CashOrNothingOption", {{"payout", "payout"}});

    auto asset = nb::class_<AssetOrNothingOption>(
        module, "AssetOrNothingOption", "Immutable validated asset-or-nothing option.")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonDate effective_date,
                        PythonDate expiry_date) {
                 return unwrap(make_asset_or_nothing_option(
                     type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                     calendar_date(expiry_date, "expiry_date")));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
             "Create a validated asset-or-nothing option.");
    bind_common_option_properties(asset, "AssetOrNothingOption");

    bind_average_option(module, "GeometricAveragePriceOption", &make_geometric_average_option);
    bind_average_option(module, "ArithmeticAveragePriceOption", &make_arithmetic_average_option);

    auto barrier = nb::class_<BarrierOption>(
        module, "BarrierOption", "Immutable validated barrier option.")
        .def(nb::new_([](OptionType type, PythonReal strike, PythonDate effective_date,
                        PythonDate expiry_date, PythonReal barrier, BarrierType BarrierType,
                        PythonReal rebate, RebateTiming RebateTiming,
                        ObservationMode ObservationMode, PythonDateSequence observation_dates) {
                 return unwrap(make_barrier_option({
                     type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                     calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                     BarrierType, real_number(rebate, "rebate"), RebateTiming, ObservationMode,
                     date_sequence(observation_dates, "observation_dates")}));
             }),
             nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
             "barrier_level"_a, "barrier_type"_a, "rebate"_a = BarrierOptionTerms{}.rebate,
             "rebate_timing"_a = BarrierOptionTerms{}.rebate_timing,
             "observation_mode"_a = BarrierOptionTerms{}.observation_mode,
             "observation_dates"_a = nb::make_tuple(),
             "Create a validated barrier option.")
        .def_prop_ro("option_type", &BarrierOption::option_type)
        .def_prop_ro("strike", &BarrierOption::strike)
        .def_prop_ro("effective_date", [](const BarrierOption& value) { return python_date(value.effective_date()); })
        .def_prop_ro("expiry_date", [](const BarrierOption& value) { return python_date(value.expiry_date()); })
        .def_prop_ro("barrier_level", &BarrierOption::barrier_level)
        .def_prop_ro("barrier_type", &BarrierOption::barrier_type)
        .def_prop_ro("rebate", &BarrierOption::rebate)
        .def_prop_ro("rebate_timing", &BarrierOption::rebate_timing)
        .def_prop_ro("observation_mode", &BarrierOption::observation_mode)
        .def_prop_ro("observation_dates", [](const BarrierOption& value) {
            PythonDateList output;
            for (const Date item : value.observation_dates()) output.append(python_date(item));
            return output;
        });
    bind_value_equality(barrier);
    bind_repr(barrier, "BarrierOption",
              {{"option_type", "option_type"}, {"strike", "strike"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"},
               {"barrier_level", "barrier_level"}, {"barrier_type", "barrier_type"},
               {"rebate", "rebate"}, {"rebate_timing", "rebate_timing"},
               {"observation_mode", "observation_mode"},
               {"observation_dates", "observation_dates"}});

    auto binary_barrier = nb::class_<BinaryBarrierOption>(
        module, "BinaryBarrierOption", "Immutable validated strike-based binary barrier option.")
        .def_prop_ro("option_type", &BinaryBarrierOption::option_type)
        .def_prop_ro("strike", &BinaryBarrierOption::strike)
        .def_prop_ro("effective_date", [](const BinaryBarrierOption& value) { return python_date(value.effective_date()); })
        .def_prop_ro("expiry_date", [](const BinaryBarrierOption& value) { return python_date(value.expiry_date()); })
        .def_prop_ro("barrier_level", &BinaryBarrierOption::barrier_level)
        .def_prop_ro("barrier_type", &BinaryBarrierOption::barrier_type)
        .def_prop_ro("payoff_type", &BinaryBarrierOption::payoff_type)
        .def_prop_ro("payout", [](const BinaryBarrierOption& value) -> std::optional<double> {
            if (const auto* cash = std::get_if<CashOrNothingPayoff>(&value.payoff()))
                return cash->payout();
            return std::nullopt;
        })
        .def_prop_ro("observation_mode", &BinaryBarrierOption::observation_mode)
        .def_prop_ro("observation_dates", [](const BinaryBarrierOption& value) {
            PythonDateList output;
            for (const Date item : value.observation_dates()) output.append(python_date(item));
            return output;
        });
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
                  PythonDate expiry_date, PythonReal barrier, BarrierType BarrierType,
                  PythonReal payout, ObservationMode ObservationMode,
                  PythonDateSequence observation_dates) {
                   return unwrap(make_cash_binary_barrier_option(
                       {type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                        calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                        BarrierType, ObservationMode, date_sequence(observation_dates, "observation_dates")},
                       real_number(payout, "payout")));
               },
               nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
               "barrier_level"_a, "barrier_type"_a, "payout"_a,
               "observation_mode"_a = BinaryBarrierTerms{}.observation_mode,
               "observation_dates"_a = nb::make_tuple(),
               "Create a validated cash binary barrier option.");
    module.def("asset_binary_barrier_option",
               [](OptionType type, PythonReal strike, PythonDate effective_date,
                  PythonDate expiry_date, PythonReal barrier, BarrierType BarrierType,
                  ObservationMode ObservationMode, PythonDateSequence observation_dates) {
                   return unwrap(make_asset_binary_barrier_option(
                       {type, real_number(strike, "strike"), calendar_date(effective_date, "effective_date"),
                        calendar_date(expiry_date, "expiry_date"), real_number(barrier, "barrier_level"),
                        BarrierType, ObservationMode, date_sequence(observation_dates, "observation_dates")}));
               },
               nb::kw_only(), "option_type"_a, "strike"_a, "effective_date"_a, "expiry_date"_a,
               "barrier_level"_a, "barrier_type"_a,
               "observation_mode"_a = BinaryBarrierTerms{}.observation_mode,
               "observation_dates"_a = nb::make_tuple(),
               "Create a validated asset binary barrier option.");

    auto touch = nb::class_<TouchOption>(
        module, "TouchOption", "Immutable validated one-touch or no-touch option.")
        .def_prop_ro("effective_date", [](const TouchOption& value) { return python_date(value.effective_date()); })
        .def_prop_ro("expiry_date", [](const TouchOption& value) { return python_date(value.expiry_date()); })
        .def_prop_ro("barrier_level", &TouchOption::barrier_level)
        .def_prop_ro("is_one_touch", &TouchOption::is_one_touch)
        .def_prop_ro("is_up", &TouchOption::is_up)
        .def_prop_ro("payoff_type", &TouchOption::payoff_type)
        .def_prop_ro("payout", [](const TouchOption& value) -> std::optional<double> {
            if (const auto* cash = std::get_if<CashOrNothingPayoff>(&value.payoff()))
                return cash->payout();
            return std::nullopt;
        })
        .def_prop_ro("settlement_timing", &TouchOption::settlement_timing)
        .def_prop_ro("observation_mode", &TouchOption::observation_mode)
        .def_prop_ro("observation_dates", [](const TouchOption& value) {
            PythonDateList output;
            for (const Date item : value.observation_dates()) output.append(python_date(item));
            return output;
        });
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
                  PythonReal payout, SettlementTiming SettlementTiming,
                  ObservationMode ObservationMode, PythonDateSequence observation_dates) {
                   return unwrap(make_cash_one_touch_up(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                       SettlementTiming,
                       ObservationMode, date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a,
               "settlement_timing"_a = SettlementTiming::at_expiry,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               "Create a cash one-touch with an upper barrier.");
    module.def("cash_one_touch_down",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  PythonReal payout, SettlementTiming SettlementTiming,
                  ObservationMode ObservationMode, PythonDateSequence observation_dates) {
                   return unwrap(make_cash_one_touch_down(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                       SettlementTiming,
                       ObservationMode, date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a,
               "settlement_timing"_a = SettlementTiming::at_expiry,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               "Create a cash one-touch with a lower barrier.");
    module.def("cash_no_touch_up",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonReal payout,
                  ObservationMode ObservationMode, PythonDateSequence observation_dates) {
                   return unwrap(make_cash_no_touch_up(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                       ObservationMode, date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               "Create a cash no-touch with an upper barrier.");
    module.def("cash_no_touch_down",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier, PythonReal payout,
                  ObservationMode ObservationMode, PythonDateSequence observation_dates) {
                   return unwrap(make_cash_no_touch_down(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), real_number(payout, "payout"),
                       ObservationMode, date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a, "payout"_a,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               "Create a cash no-touch with a lower barrier.");
    module.def("asset_one_touch_up",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  SettlementTiming SettlementTiming, ObservationMode ObservationMode,
                  PythonDateSequence observation_dates) {
                   return unwrap(make_asset_one_touch_up(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), SettlementTiming, ObservationMode,
                       date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a,
               "settlement_timing"_a = SettlementTiming::at_expiry,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               "Create an asset one-touch with an upper barrier.");
    module.def("asset_one_touch_down",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  SettlementTiming SettlementTiming, ObservationMode ObservationMode,
                  PythonDateSequence observation_dates) {
                   return unwrap(make_asset_one_touch_down(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), SettlementTiming, ObservationMode,
                       date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a,
               "settlement_timing"_a = SettlementTiming::at_expiry,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               "Create an asset one-touch with a lower barrier.");
    module.def("asset_no_touch_up",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  ObservationMode ObservationMode, PythonDateSequence observation_dates) {
                   return unwrap(make_asset_no_touch_up(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), ObservationMode,
                       date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               "Create an asset no-touch with an upper barrier.");
    module.def("asset_no_touch_down",
               [](PythonDate effective_date, PythonDate expiry_date, PythonReal barrier,
                  ObservationMode ObservationMode, PythonDateSequence observation_dates) {
                   return unwrap(make_asset_no_touch_down(
                       calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date"),
                       real_number(barrier, "barrier_level"), ObservationMode,
                       date_sequence(observation_dates, "observation_dates")));
               },
               nb::kw_only(), "effective_date"_a, "expiry_date"_a, "barrier_level"_a,
               "observation_mode"_a = ObservationMode::continuous,
               "observation_dates"_a = nb::make_tuple(),
               "Create an asset no-touch with a lower barrier.");

    auto accumulator = nb::class_<Accumulator>(
        module, "Accumulator", "Immutable validated accumulator contract.")
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
             "expiry_date"_a, "Create a validated accumulator contract.")
        .def_prop_ro("strike", &Accumulator::strike)
        .def_prop_ro("knock_out_level", &Accumulator::knock_out_level)
        .def_prop_ro("daily_quantity", &Accumulator::daily_quantity)
        .def_prop_ro("acceleration_factor", &Accumulator::acceleration_factor)
        .def_prop_ro("accumulated_quantity", &Accumulator::accumulated_quantity)
        .def_prop_ro("effective_date", [](const Accumulator& value) { return python_date(value.effective_date()); })
        .def_prop_ro("expiry_date", [](const Accumulator& value) { return python_date(value.expiry_date()); });
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
