#include "_binding.hpp"

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

template <typename Note>
void bind_note_properties(nb::class_<Note>& binding)
{
    binding.def_prop_ro("initial_spot", &Note::initial_spot)
        .def_prop_ro("knock_out_levels", &Note::knock_out_levels)
        .def_prop_ro("upper_strike", &Note::upper_strike)
        .def_prop_ro("lower_strike", &Note::lower_strike)
        .def_prop_ro("observation_dates", [](const Note& note) {
            PythonDateList output;
            for (const Date value : note.observation_dates()) output.append(python_date(value));
            return output;
        })
        .def_prop_ro("principal_ratio", &Note::principal_ratio)
        .def_prop_ro("touch_status", &Note::touch_status)
        .def_prop_ro("effective_date", [](const Note& note) { return python_date(note.effective_date()); })
        .def_prop_ro("expiry_date", [](const Note& note) { return python_date(note.expiry_date()); });
}

template <typename Note>
void bind_knock_in_properties(nb::class_<Note>& binding)
{
    bind_note_properties(binding);
    binding.def_prop_ro("knock_in_level", &Note::knock_in_level)
        .def_prop_ro("knock_in_observation_mode", &Note::knock_in_observation_mode);
}

template <typename Terms, typename Factory>
void bind_basic_preset(nb::module_& module, const char* name, Factory factory)
{
    module.def(
        name,
        [factory](PythonReal coupon_rate, PythonReal initial_spot,
                  PythonReal knock_in_level, PythonReal knock_out_level,
                  PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
                  BarrierTouchStatus touch_status, PythonReal principal_ratio) {
            return unwrap(factory(Terms{
                .coupon_rate = real_number(coupon_rate, "coupon_rate"),
                .initial_spot = real_number(initial_spot, "initial_spot"),
                .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                .observation_dates = date_sequence(observation_dates, "observation_dates"),
                .effective_date = calendar_date(effective_date, "effective_date"),
                .expiry_date = calendar_date(expiry_date, "expiry_date"),
                .touch_status = touch_status,
                .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "knock_out_level"_a, "observation_dates"_a, "effective_date"_a, "expiry_date"_a,
        "touch_status"_a = SnowballTerms{}.touch_status,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        "Create a validated snowball preset.");
}

void bind_presets(nb::module_& module)
{
    bind_basic_preset<StandardSnowballTerms>(module, "standard_snowball", &make_standard_snowball);
    bind_basic_preset<EuropeanSnowballTerms>(module, "european_snowball", &make_european_snowball);
    module.def(
        "step_down_snowball",
        [](PythonReal coupon_rate, PythonReal initial_spot, PythonReal knock_in_level,
           PythonReal initial_knock_out_level, PythonReal knock_out_level_decrement,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           BarrierTouchStatus touch_status, PythonReal principal_ratio) {
            return unwrap(make_step_down_snowball({
                .coupon_rate = real_number(coupon_rate, "coupon_rate"),
                .initial_spot = real_number(initial_spot, "initial_spot"),
                .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                .initial_knock_out_level = real_number(initial_knock_out_level, "initial_knock_out_level"),
                .knock_out_level_decrement = real_number(knock_out_level_decrement, "knock_out_level_decrement"),
                .observation_dates = date_sequence(observation_dates, "observation_dates"),
                .effective_date = calendar_date(effective_date, "effective_date"),
                .expiry_date = calendar_date(expiry_date, "expiry_date"),
                .touch_status = touch_status,
                .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "initial_knock_out_level"_a, "knock_out_level_decrement"_a, "observation_dates"_a, "effective_date"_a,
        "expiry_date"_a, "touch_status"_a = SnowballTerms{}.touch_status,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        "Create a snowball with linearly decreasing knock-out barriers.");
    module.def(
        "both_down_snowball",
        [](PythonReal coupon_start, PythonReal coupon_step, PythonReal initial_spot,
           PythonReal knock_in_level, PythonReal initial_knock_out_level, PythonReal knock_out_level_decrement,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           BarrierTouchStatus touch_status, PythonReal principal_ratio) {
            return unwrap(make_both_down_snowball({
                .coupon_start = real_number(coupon_start, "coupon_start"),
                .coupon_step = real_number(coupon_step, "coupon_step"),
                .initial_spot = real_number(initial_spot, "initial_spot"),
                .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                .initial_knock_out_level = real_number(initial_knock_out_level, "initial_knock_out_level"),
                .knock_out_level_decrement = real_number(knock_out_level_decrement, "knock_out_level_decrement"),
                .observation_dates = date_sequence(observation_dates, "observation_dates"),
                .effective_date = calendar_date(effective_date, "effective_date"),
                .expiry_date = calendar_date(expiry_date, "expiry_date"),
                .touch_status = touch_status,
                .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_start"_a, "coupon_step"_a, "initial_spot"_a,
        "knock_in_level"_a, "initial_knock_out_level"_a, "knock_out_level_decrement"_a,
        "observation_dates"_a, "effective_date"_a, "expiry_date"_a,
        "touch_status"_a = SnowballTerms{}.touch_status,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        "Create a snowball with decreasing coupons and knock-out barriers.");
    module.def(
        "dual_coupon_snowball",
        [](PythonReal knock_out_coupon, PythonReal maturity_coupon,
           PythonReal initial_spot, PythonReal knock_in_level, PythonReal knock_out_level,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           BarrierTouchStatus touch_status, PythonReal principal_ratio) {
            return unwrap(make_dual_coupon_snowball({
                .knock_out_coupon = real_number(knock_out_coupon, "knock_out_coupon"),
                .maturity_coupon = real_number(maturity_coupon, "maturity_coupon"),
                .initial_spot = real_number(initial_spot, "initial_spot"),
                .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                .observation_dates = date_sequence(observation_dates, "observation_dates"),
                .effective_date = calendar_date(effective_date, "effective_date"),
                .expiry_date = calendar_date(expiry_date, "expiry_date"),
                .touch_status = touch_status,
                .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "knock_out_coupon"_a, "maturity_coupon"_a,
        "initial_spot"_a, "knock_in_level"_a, "knock_out_level"_a,
        "observation_dates"_a, "effective_date"_a, "expiry_date"_a,
        "touch_status"_a = SnowballTerms{}.touch_status,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        "Create a snowball with separate knock-out and maturity coupons.");
    module.def(
        "parachute_snowball",
        [](PythonReal coupon_rate, PythonReal initial_spot, PythonReal knock_in_level,
           PythonReal knock_out_level, PythonReal final_knock_out_level,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           BarrierTouchStatus touch_status, PythonReal principal_ratio) {
            return unwrap(make_parachute_snowball({
                .coupon_rate = real_number(coupon_rate, "coupon_rate"),
                .initial_spot = real_number(initial_spot, "initial_spot"),
                .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                .final_knock_out_level = real_number(final_knock_out_level, "final_knock_out_level"),
                .observation_dates = date_sequence(observation_dates, "observation_dates"),
                .effective_date = calendar_date(effective_date, "effective_date"),
                .expiry_date = calendar_date(expiry_date, "expiry_date"),
                .touch_status = touch_status,
                .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "knock_out_level"_a, "final_knock_out_level"_a, "observation_dates"_a,
        "effective_date"_a, "expiry_date"_a, "touch_status"_a = SnowballTerms{}.touch_status,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        "Create a snowball with a distinct final knock-out barrier.");
    module.def(
        "out_of_the_money_snowball",
        [](PythonReal coupon_rate, PythonReal initial_spot, PythonReal knock_in_level,
           PythonReal knock_out_level, PythonReal upper_strike,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           BarrierTouchStatus touch_status, PythonReal principal_ratio) {
            return unwrap(make_otm_snowball({
                .coupon_rate = real_number(coupon_rate, "coupon_rate"),
                .initial_spot = real_number(initial_spot, "initial_spot"),
                .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                .upper_strike = real_number(upper_strike, "upper_strike"),
                .observation_dates = date_sequence(observation_dates, "observation_dates"),
                .effective_date = calendar_date(effective_date, "effective_date"),
                .expiry_date = calendar_date(expiry_date, "expiry_date"),
                .touch_status = touch_status,
                .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "knock_out_level"_a, "upper_strike"_a, "observation_dates"_a, "effective_date"_a,
        "expiry_date"_a, "touch_status"_a = SnowballTerms{}.touch_status,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        "Create an out-of-the-money snowball preset.");
    module.def(
        "loss_capped_snowball",
        [](PythonReal coupon_rate, PythonReal initial_spot, PythonReal knock_in_level,
           PythonReal knock_out_level, PythonReal lower_strike,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           BarrierTouchStatus touch_status, PythonReal principal_ratio) {
            return unwrap(make_loss_capped_snowball({
                .coupon_rate = real_number(coupon_rate, "coupon_rate"),
                .initial_spot = real_number(initial_spot, "initial_spot"),
                .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                .lower_strike = real_number(lower_strike, "lower_strike"),
                .observation_dates = date_sequence(observation_dates, "observation_dates"),
                .effective_date = calendar_date(effective_date, "effective_date"),
                .expiry_date = calendar_date(expiry_date, "expiry_date"),
                .touch_status = touch_status,
                .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "knock_out_level"_a, "lower_strike"_a, "observation_dates"_a, "effective_date"_a,
        "expiry_date"_a, "touch_status"_a = SnowballTerms{}.touch_status,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        "Create a loss-capped snowball preset.");
}

} // namespace

void bind_structured_instruments(nb::module_& module)
{
    auto snowball = nb::class_<SnowballOption>(
        module, "SnowballOption", "Immutable validated snowball option.")
        .def(nb::new_([](PythonRealSequence knock_out_coupon_rates,
                        PythonReal maturity_coupon_rate, PythonReal initial_spot,
                        PythonReal knock_in_level, PythonRealSequence knock_out_levels,
                        PythonReal upper_strike, PythonReal lower_strike,
                        PythonDateSequence observation_dates,
                        KnockInObservationMode knock_in_observation_mode, BarrierTouchStatus touch_status,
                        PythonReal principal_ratio, PythonDate effective_date, PythonDate expiry_date) {
                 return unwrap(make_snowball_option({
                     real_sequence(knock_out_coupon_rates, "knock_out_coupon_rates"),
                     real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                     real_number(initial_spot, "initial_spot"),
                     real_number(knock_in_level, "knock_in_level"),
                     real_sequence(knock_out_levels, "knock_out_levels"),
                     real_number(upper_strike, "upper_strike"),
                     real_number(lower_strike, "lower_strike"),
                     date_sequence(observation_dates, "observation_dates"), knock_in_observation_mode, touch_status,
                     real_number(principal_ratio, "principal_ratio"),
                     calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date")}));
             }),
             nb::kw_only(), "knock_out_coupon_rates"_a, "maturity_coupon_rate"_a,
             "initial_spot"_a, "knock_in_level"_a, "knock_out_levels"_a,
             "upper_strike"_a, "lower_strike"_a, "observation_dates"_a, "knock_in_observation_mode"_a,
             "touch_status"_a = SnowballTerms{}.touch_status,
             "principal_ratio"_a = SnowballTerms{}.principal_ratio,
             "effective_date"_a, "expiry_date"_a, "Create a validated snowball option.")
        .def_prop_ro("knock_out_coupon_rates", &SnowballOption::knock_out_coupon_rates)
        .def_prop_ro("maturity_coupon_rate", &SnowballOption::maturity_coupon_rate);
    bind_knock_in_properties(snowball);
    bind_value_equality(snowball);
    bind_repr(snowball, "SnowballOption",
              {{"knock_out_coupon_rates", "knock_out_coupon_rates"},
               {"maturity_coupon_rate", "maturity_coupon_rate"},
               {"initial_spot", "initial_spot"}, {"knock_in_level", "knock_in_level"},
               {"knock_out_levels", "knock_out_levels"},
               {"upper_strike", "upper_strike"}, {"lower_strike", "lower_strike"},
               {"observation_dates", "observation_dates"},
               {"knock_in_observation_mode", "knock_in_observation_mode"}, {"touch_status", "touch_status"},
               {"principal_ratio", "principal_ratio"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"}});

    auto binary = nb::class_<BinarySnowballOption>(
        module, "BinarySnowballOption", "Immutable validated binary snowball option.")
        .def(nb::new_([](PythonRealSequence knock_out_coupon_rates,
                        PythonReal maturity_coupon_rate, PythonReal initial_spot,
                        PythonRealSequence knock_out_levels, PythonReal upper_strike,
                        PythonReal lower_strike, PythonDateSequence observation_dates,
                        BarrierTouchStatus touch_status, PythonReal principal_ratio,
                        PythonDate effective_date, PythonDate expiry_date) {
                 return unwrap(make_binary_snowball_option({
                     real_sequence(knock_out_coupon_rates, "knock_out_coupon_rates"),
                     real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                     real_number(initial_spot, "initial_spot"),
                     real_sequence(knock_out_levels, "knock_out_levels"),
                     real_number(upper_strike, "upper_strike"),
                     real_number(lower_strike, "lower_strike"),
                     date_sequence(observation_dates, "observation_dates"), touch_status,
                     real_number(principal_ratio, "principal_ratio"),
                     calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date")}));
             }),
             nb::kw_only(), "knock_out_coupon_rates"_a, "maturity_coupon_rate"_a,
             "initial_spot"_a, "knock_out_levels"_a, "upper_strike"_a,
             "lower_strike"_a, "observation_dates"_a,
             "touch_status"_a = BinarySnowballTerms{}.touch_status,
             "principal_ratio"_a = BinarySnowballTerms{}.principal_ratio,
             "effective_date"_a, "expiry_date"_a,
             "Create a validated binary snowball option.")
        .def_prop_ro("knock_out_coupon_rates", &BinarySnowballOption::knock_out_coupon_rates)
        .def_prop_ro("maturity_coupon_rate", &BinarySnowballOption::maturity_coupon_rate);
    bind_note_properties(binary);
    bind_value_equality(binary);
    bind_repr(binary, "BinarySnowballOption",
              {{"knock_out_coupon_rates", "knock_out_coupon_rates"},
               {"maturity_coupon_rate", "maturity_coupon_rate"},
               {"initial_spot", "initial_spot"},
               {"knock_out_levels", "knock_out_levels"},
               {"upper_strike", "upper_strike"}, {"lower_strike", "lower_strike"},
               {"observation_dates", "observation_dates"}, {"touch_status", "touch_status"},
               {"principal_ratio", "principal_ratio"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"}});

    auto ternary = nb::class_<TernarySnowballOption>(
        module, "TernarySnowballOption", "Immutable validated ternary snowball option.")
        .def(nb::new_([](PythonRealSequence knock_out_coupon_rates,
                        PythonReal maturity_coupon_rate, PythonReal minimum_coupon_rate,
                        PythonReal initial_spot, PythonReal knock_in_level,
                        PythonRealSequence knock_out_levels, PythonReal upper_strike,
                        PythonReal lower_strike, PythonDateSequence observation_dates,
                        KnockInObservationMode knock_in_observation_mode, BarrierTouchStatus touch_status,
                        PythonReal principal_ratio, PythonDate effective_date, PythonDate expiry_date) {
                 return unwrap(make_ternary_snowball_option({
                     real_sequence(knock_out_coupon_rates, "knock_out_coupon_rates"),
                     real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                     real_number(minimum_coupon_rate, "minimum_coupon_rate"),
                     real_number(initial_spot, "initial_spot"),
                     real_number(knock_in_level, "knock_in_level"),
                     real_sequence(knock_out_levels, "knock_out_levels"),
                     real_number(upper_strike, "upper_strike"),
                     real_number(lower_strike, "lower_strike"),
                     date_sequence(observation_dates, "observation_dates"), knock_in_observation_mode, touch_status,
                     real_number(principal_ratio, "principal_ratio"),
                     calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date")}));
             }),
             nb::kw_only(), "knock_out_coupon_rates"_a, "maturity_coupon_rate"_a,
             "minimum_coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
             "knock_out_levels"_a, "upper_strike"_a, "lower_strike"_a,
             "observation_dates"_a, "knock_in_observation_mode"_a,
             "touch_status"_a = TernarySnowballTerms{}.touch_status,
             "principal_ratio"_a = TernarySnowballTerms{}.principal_ratio,
             "effective_date"_a, "expiry_date"_a,
             "Create a validated ternary snowball option.")
        .def_prop_ro("knock_out_coupon_rates", &TernarySnowballOption::knock_out_coupon_rates)
        .def_prop_ro("maturity_coupon_rate", &TernarySnowballOption::maturity_coupon_rate)
        .def_prop_ro("minimum_coupon_rate", &TernarySnowballOption::minimum_coupon_rate);
    bind_knock_in_properties(ternary);
    bind_value_equality(ternary);
    bind_repr(ternary, "TernarySnowballOption",
              {{"knock_out_coupon_rates", "knock_out_coupon_rates"},
               {"maturity_coupon_rate", "maturity_coupon_rate"},
               {"minimum_coupon_rate", "minimum_coupon_rate"},
               {"initial_spot", "initial_spot"}, {"knock_in_level", "knock_in_level"},
               {"knock_out_levels", "knock_out_levels"},
               {"upper_strike", "upper_strike"}, {"lower_strike", "lower_strike"},
               {"observation_dates", "observation_dates"},
               {"knock_in_observation_mode", "knock_in_observation_mode"}, {"touch_status", "touch_status"},
               {"principal_ratio", "principal_ratio"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"}});

    auto phoenix = nb::class_<PhoenixOption>(
        module, "PhoenixOption", "Immutable validated Phoenix option.")
        .def(nb::new_([](PythonReal coupon_rate, PythonReal initial_spot,
                        PythonReal knock_in_level, PythonRealSequence knock_out_levels,
                        PythonRealSequence coupon_barrier_levels, PythonReal upper_strike,
                        PythonReal lower_strike, PythonDateSequence observation_dates,
                        KnockInObservationMode knock_in_observation_mode, BarrierTouchStatus touch_status,
                        PythonReal principal_ratio, PythonDate effective_date, PythonDate expiry_date) {
                 return unwrap(make_phoenix_option({
                     real_number(coupon_rate, "coupon_rate"),
                     real_number(initial_spot, "initial_spot"),
                     real_number(knock_in_level, "knock_in_level"),
                     real_sequence(knock_out_levels, "knock_out_levels"),
                     real_sequence(coupon_barrier_levels, "coupon_barrier_levels"),
                     real_number(upper_strike, "upper_strike"),
                     real_number(lower_strike, "lower_strike"),
                     date_sequence(observation_dates, "observation_dates"), knock_in_observation_mode, touch_status,
                     real_number(principal_ratio, "principal_ratio"),
                     calendar_date(effective_date, "effective_date"), calendar_date(expiry_date, "expiry_date")}));
             }),
             nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
             "knock_out_levels"_a, "coupon_barrier_levels"_a, "upper_strike"_a,
             "lower_strike"_a, "observation_dates"_a, "knock_in_observation_mode"_a,
             "touch_status"_a = PhoenixTerms{}.touch_status,
             "principal_ratio"_a = PhoenixTerms{}.principal_ratio,
             "effective_date"_a, "expiry_date"_a, "Create a validated Phoenix option.")
        .def_prop_ro("coupon_rate", &PhoenixOption::coupon_rate)
        .def_prop_ro("coupon_barrier_levels", &PhoenixOption::coupon_barrier_levels);
    bind_knock_in_properties(phoenix);
    bind_value_equality(phoenix);
    bind_repr(phoenix, "PhoenixOption",
              {{"coupon_rate", "coupon_rate"}, {"initial_spot", "initial_spot"},
               {"knock_in_level", "knock_in_level"},
               {"knock_out_levels", "knock_out_levels"},
               {"coupon_barrier_levels", "coupon_barrier_levels"},
               {"upper_strike", "upper_strike"}, {"lower_strike", "lower_strike"},
               {"observation_dates", "observation_dates"},
               {"knock_in_observation_mode", "knock_in_observation_mode"}, {"touch_status", "touch_status"},
               {"principal_ratio", "principal_ratio"},
               {"effective_date", "effective_date"}, {"expiry_date", "expiry_date"}});

    bind_presets(module);
}

} // namespace kiyosi::python_binding
