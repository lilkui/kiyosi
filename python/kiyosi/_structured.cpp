#include "_binding.hpp"

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

template <typename Note>
void bind_note_properties(nb::class_<Note>& binding)
{
    binding.def_prop_ro("initial_price", &Note::initial_price)
        .def_prop_ro("knock_out_prices", &Note::knock_out_prices)
        .def_prop_ro("upper_strike", &Note::upper_strike)
        .def_prop_ro("lower_strike", &Note::lower_strike)
        .def_prop_ro("observation_dates", [](const Note& note) {
            PythonDateList output;
            for (const date value : note.observation_dates()) output.append(python_date(value));
            return output;
        })
        .def_prop_ro("principal_ratio", &Note::principal_ratio)
        .def_prop_ro("touch_status", &Note::touch_status)
        .def_prop_ro("effective", [](const Note& note) { return python_date(note.effective()); })
        .def_prop_ro("expiry", [](const Note& note) { return python_date(note.expiry()); });
}

template <typename Note>
void bind_knock_in_properties(nb::class_<Note>& binding)
{
    bind_note_properties(binding);
    binding.def_prop_ro("knock_in_price", &Note::knock_in_price)
        .def_prop_ro("knock_in_frequency", &Note::knock_in_frequency);
}

template <typename Factory>
void bind_basic_preset(nb::module_& module, const char* name, Factory factory)
{
    module.def(
        name,
        [factory](PythonReal coupon_rate, PythonReal initial_price,
                  PythonReal knock_in_price, PythonReal knock_out_price,
                  PythonDateSequence observations, PythonDate effective, PythonDate expiry,
                  barrier_touch_status touch_status, PythonReal principal_ratio) {
            return unwrap(factory(
                real_number(coupon_rate, "coupon_rate"),
                real_number(initial_price, "initial_price"),
                real_number(knock_in_price, "knock_in_price"),
                real_number(knock_out_price, "knock_out_price"),
                date_sequence(observations, "observations"), calendar_date(effective, "effective"),
                calendar_date(expiry, "expiry"), touch_status,
                real_number(principal_ratio, "principal_ratio")));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_price"_a, "knock_in_price"_a,
        "knock_out_price"_a, "observations"_a, "effective"_a, "expiry"_a,
        "touch_status"_a = barrier_touch_status::none, "principal_ratio"_a = 1.0);
}

void bind_presets(nb::module_& module)
{
    bind_basic_preset(module, "standard_snowball", &make_standard_snowball);
    bind_basic_preset(module, "european_snowball", &make_european_snowball);
    module.def(
        "step_down_snowball",
        [](PythonReal coupon_rate, PythonReal initial_price, PythonReal knock_in_price,
           PythonReal knock_out_start, PythonReal knock_out_step,
           PythonDateSequence observations, PythonDate effective, PythonDate expiry,
           barrier_touch_status touch_status, PythonReal principal_ratio) {
            return unwrap(make_step_down_snowball(
                real_number(coupon_rate, "coupon_rate"),
                real_number(initial_price, "initial_price"),
                real_number(knock_in_price, "knock_in_price"),
                real_number(knock_out_start, "knock_out_start"),
                real_number(knock_out_step, "knock_out_step"),
                date_sequence(observations, "observations"), calendar_date(effective, "effective"),
                calendar_date(expiry, "expiry"), touch_status,
                real_number(principal_ratio, "principal_ratio")));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_price"_a, "knock_in_price"_a,
        "knock_out_start"_a, "knock_out_step"_a, "observations"_a, "effective"_a,
        "expiry"_a, "touch_status"_a = barrier_touch_status::none,
        "principal_ratio"_a = 1.0);
    module.def(
        "both_down_snowball",
        [](PythonReal coupon_start, PythonReal coupon_step, PythonReal initial_price,
           PythonReal knock_in_price, PythonReal knock_out_start, PythonReal knock_out_step,
           PythonDateSequence observations, PythonDate effective, PythonDate expiry,
           barrier_touch_status touch_status, PythonReal principal_ratio) {
            return unwrap(make_both_down_snowball(
                real_number(coupon_start, "coupon_start"),
                real_number(coupon_step, "coupon_step"),
                real_number(initial_price, "initial_price"),
                real_number(knock_in_price, "knock_in_price"),
                real_number(knock_out_start, "knock_out_start"),
                real_number(knock_out_step, "knock_out_step"),
                date_sequence(observations, "observations"), calendar_date(effective, "effective"),
                calendar_date(expiry, "expiry"), touch_status,
                real_number(principal_ratio, "principal_ratio")));
        },
        nb::kw_only(), "coupon_start"_a, "coupon_step"_a, "initial_price"_a,
        "knock_in_price"_a, "knock_out_start"_a, "knock_out_step"_a,
        "observations"_a, "effective"_a, "expiry"_a,
        "touch_status"_a = barrier_touch_status::none, "principal_ratio"_a = 1.0);
    module.def(
        "dual_coupon_snowball",
        [](PythonReal knock_out_coupon, PythonReal maturity_coupon,
           PythonReal initial_price, PythonReal knock_in_price, PythonReal knock_out_price,
           PythonDateSequence observations, PythonDate effective, PythonDate expiry,
           barrier_touch_status touch_status, PythonReal principal_ratio) {
            return unwrap(make_dual_coupon_snowball(
                real_number(knock_out_coupon, "knock_out_coupon"),
                real_number(maturity_coupon, "maturity_coupon"),
                real_number(initial_price, "initial_price"),
                real_number(knock_in_price, "knock_in_price"),
                real_number(knock_out_price, "knock_out_price"),
                date_sequence(observations, "observations"), calendar_date(effective, "effective"),
                calendar_date(expiry, "expiry"), touch_status,
                real_number(principal_ratio, "principal_ratio")));
        },
        nb::kw_only(), "knock_out_coupon"_a, "maturity_coupon"_a,
        "initial_price"_a, "knock_in_price"_a, "knock_out_price"_a,
        "observations"_a, "effective"_a, "expiry"_a,
        "touch_status"_a = barrier_touch_status::none, "principal_ratio"_a = 1.0);
    module.def(
        "parachute_snowball",
        [](PythonReal coupon_rate, PythonReal initial_price, PythonReal knock_in_price,
           PythonReal knock_out_price, PythonReal final_knock_out_price,
           PythonDateSequence observations, PythonDate effective, PythonDate expiry,
           barrier_touch_status touch_status, PythonReal principal_ratio) {
            return unwrap(make_parachute_snowball(
                real_number(coupon_rate, "coupon_rate"),
                real_number(initial_price, "initial_price"),
                real_number(knock_in_price, "knock_in_price"),
                real_number(knock_out_price, "knock_out_price"),
                real_number(final_knock_out_price, "final_knock_out_price"),
                date_sequence(observations, "observations"), calendar_date(effective, "effective"),
                calendar_date(expiry, "expiry"), touch_status,
                real_number(principal_ratio, "principal_ratio")));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_price"_a, "knock_in_price"_a,
        "knock_out_price"_a, "final_knock_out_price"_a, "observations"_a,
        "effective"_a, "expiry"_a, "touch_status"_a = barrier_touch_status::none,
        "principal_ratio"_a = 1.0);
    module.def(
        "otm_snowball",
        [](PythonReal coupon_rate, PythonReal initial_price, PythonReal knock_in_price,
           PythonReal knock_out_price, PythonReal upper_strike,
           PythonDateSequence observations, PythonDate effective, PythonDate expiry,
           barrier_touch_status touch_status, PythonReal principal_ratio) {
            return unwrap(make_otm_snowball(
                real_number(coupon_rate, "coupon_rate"),
                real_number(initial_price, "initial_price"),
                real_number(knock_in_price, "knock_in_price"),
                real_number(knock_out_price, "knock_out_price"),
                real_number(upper_strike, "upper_strike"),
                date_sequence(observations, "observations"), calendar_date(effective, "effective"),
                calendar_date(expiry, "expiry"), touch_status,
                real_number(principal_ratio, "principal_ratio")));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_price"_a, "knock_in_price"_a,
        "knock_out_price"_a, "upper_strike"_a, "observations"_a, "effective"_a,
        "expiry"_a, "touch_status"_a = barrier_touch_status::none,
        "principal_ratio"_a = 1.0);
    module.def(
        "loss_capped_snowball",
        [](PythonReal coupon_rate, PythonReal initial_price, PythonReal knock_in_price,
           PythonReal knock_out_price, PythonReal lower_strike,
           PythonDateSequence observations, PythonDate effective, PythonDate expiry,
           barrier_touch_status touch_status, PythonReal principal_ratio) {
            return unwrap(make_loss_capped_snowball(
                real_number(coupon_rate, "coupon_rate"),
                real_number(initial_price, "initial_price"),
                real_number(knock_in_price, "knock_in_price"),
                real_number(knock_out_price, "knock_out_price"),
                real_number(lower_strike, "lower_strike"),
                date_sequence(observations, "observations"), calendar_date(effective, "effective"),
                calendar_date(expiry, "expiry"), touch_status,
                real_number(principal_ratio, "principal_ratio")));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_price"_a, "knock_in_price"_a,
        "knock_out_price"_a, "lower_strike"_a, "observations"_a, "effective"_a,
        "expiry"_a, "touch_status"_a = barrier_touch_status::none,
        "principal_ratio"_a = 1.0);
}

} // namespace

void bind_structured_instruments(nb::module_& module)
{
    auto snowball = nb::class_<SnowballOption>(module, "SnowballOption")
        .def(nb::new_([](PythonRealSequence knock_out_coupon_rates,
                        PythonReal maturity_coupon_rate, PythonReal initial_price,
                        PythonReal knock_in_price, PythonRealSequence knock_out_prices,
                        PythonReal upper_strike, PythonReal lower_strike,
                        PythonDateSequence observations,
                        observation_frequency frequency, barrier_touch_status touch_status,
                        PythonReal principal_ratio, PythonDate effective, PythonDate expiry) {
                 return unwrap(make_snowball_option({
                     real_sequence(knock_out_coupon_rates, "knock_out_coupon_rates"),
                     real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                     real_number(initial_price, "initial_price"),
                     real_number(knock_in_price, "knock_in_price"),
                     real_sequence(knock_out_prices, "knock_out_prices"),
                     real_number(upper_strike, "upper_strike"),
                     real_number(lower_strike, "lower_strike"),
                     date_sequence(observations, "observations"), frequency, touch_status,
                     real_number(principal_ratio, "principal_ratio"),
                     calendar_date(effective, "effective"), calendar_date(expiry, "expiry")}));
             }),
             nb::kw_only(), "knock_out_coupon_rates"_a, "maturity_coupon_rate"_a,
             "initial_price"_a, "knock_in_price"_a, "knock_out_prices"_a,
             "upper_strike"_a, "lower_strike"_a, "observations"_a, "frequency"_a,
             "touch_status"_a = barrier_touch_status::none, "principal_ratio"_a = 1.0,
             "effective"_a, "expiry"_a)
        .def_prop_ro("knock_out_coupon_rates", &SnowballOption::knock_out_coupon_rates)
        .def_prop_ro("maturity_coupon_rate", &SnowballOption::maturity_coupon_rate)
        .def("with_coupon_rate", [](const SnowballOption& option, PythonReal coupon) {
            return unwrap(option.with_coupon_rate(real_number(coupon, "coupon")));
        }, "coupon"_a);
    bind_knock_in_properties(snowball);

    auto binary = nb::class_<BinarySnowballOption>(module, "BinarySnowballOption")
        .def(nb::new_([](PythonRealSequence knock_out_coupon_rates,
                        PythonReal maturity_coupon_rate, PythonReal initial_price,
                        PythonRealSequence knock_out_prices, PythonReal upper_strike,
                        PythonReal lower_strike, PythonDateSequence observations,
                        barrier_touch_status touch_status, PythonReal principal_ratio,
                        PythonDate effective, PythonDate expiry) {
                 return unwrap(make_binary_snowball_option({
                     real_sequence(knock_out_coupon_rates, "knock_out_coupon_rates"),
                     real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                     real_number(initial_price, "initial_price"),
                     real_sequence(knock_out_prices, "knock_out_prices"),
                     real_number(upper_strike, "upper_strike"),
                     real_number(lower_strike, "lower_strike"),
                     date_sequence(observations, "observations"), touch_status,
                     real_number(principal_ratio, "principal_ratio"),
                     calendar_date(effective, "effective"), calendar_date(expiry, "expiry")}));
             }),
             nb::kw_only(), "knock_out_coupon_rates"_a, "maturity_coupon_rate"_a,
             "initial_price"_a, "knock_out_prices"_a, "upper_strike"_a,
             "lower_strike"_a, "observations"_a,
             "touch_status"_a = barrier_touch_status::none, "principal_ratio"_a = 1.0,
             "effective"_a, "expiry"_a)
        .def_prop_ro("knock_out_coupon_rates", &BinarySnowballOption::knock_out_coupon_rates)
        .def_prop_ro("maturity_coupon_rate", &BinarySnowballOption::maturity_coupon_rate);
    bind_note_properties(binary);

    auto ternary = nb::class_<TernarySnowballOption>(module, "TernarySnowballOption")
        .def(nb::new_([](PythonRealSequence knock_out_coupon_rates,
                        PythonReal maturity_coupon_rate, PythonReal minimal_coupon_rate,
                        PythonReal initial_price, PythonReal knock_in_price,
                        PythonRealSequence knock_out_prices, PythonReal upper_strike,
                        PythonReal lower_strike, PythonDateSequence observations,
                        observation_frequency frequency, barrier_touch_status touch_status,
                        PythonReal principal_ratio, PythonDate effective, PythonDate expiry) {
                 return unwrap(make_ternary_snowball_option({
                     real_sequence(knock_out_coupon_rates, "knock_out_coupon_rates"),
                     real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                     real_number(minimal_coupon_rate, "minimal_coupon_rate"),
                     real_number(initial_price, "initial_price"),
                     real_number(knock_in_price, "knock_in_price"),
                     real_sequence(knock_out_prices, "knock_out_prices"),
                     real_number(upper_strike, "upper_strike"),
                     real_number(lower_strike, "lower_strike"),
                     date_sequence(observations, "observations"), frequency, touch_status,
                     real_number(principal_ratio, "principal_ratio"),
                     calendar_date(effective, "effective"), calendar_date(expiry, "expiry")}));
             }),
             nb::kw_only(), "knock_out_coupon_rates"_a, "maturity_coupon_rate"_a,
             "minimal_coupon_rate"_a, "initial_price"_a, "knock_in_price"_a,
             "knock_out_prices"_a, "upper_strike"_a, "lower_strike"_a,
             "observations"_a, "frequency"_a,
             "touch_status"_a = barrier_touch_status::none, "principal_ratio"_a = 1.0,
             "effective"_a, "expiry"_a)
        .def_prop_ro("knock_out_coupon_rates", &TernarySnowballOption::knock_out_coupon_rates)
        .def_prop_ro("maturity_coupon_rate", &TernarySnowballOption::maturity_coupon_rate)
        .def_prop_ro("minimal_coupon_rate", &TernarySnowballOption::minimal_coupon_rate);
    bind_knock_in_properties(ternary);

    auto phoenix = nb::class_<PhoenixOption>(module, "PhoenixOption")
        .def(nb::new_([](PythonReal coupon_rate, PythonReal initial_price,
                        PythonReal knock_in_price, PythonRealSequence knock_out_prices,
                        PythonRealSequence coupon_barriers, PythonReal upper_strike,
                        PythonReal lower_strike, PythonDateSequence observations,
                        observation_frequency frequency, barrier_touch_status touch_status,
                        PythonReal principal_ratio, PythonDate effective, PythonDate expiry) {
                 return unwrap(make_phoenix_option({
                     real_number(coupon_rate, "coupon_rate"),
                     real_number(initial_price, "initial_price"),
                     real_number(knock_in_price, "knock_in_price"),
                     real_sequence(knock_out_prices, "knock_out_prices"),
                     real_sequence(coupon_barriers, "coupon_barriers"),
                     real_number(upper_strike, "upper_strike"),
                     real_number(lower_strike, "lower_strike"),
                     date_sequence(observations, "observations"), frequency, touch_status,
                     real_number(principal_ratio, "principal_ratio"),
                     calendar_date(effective, "effective"), calendar_date(expiry, "expiry")}));
             }),
             nb::kw_only(), "coupon_rate"_a, "initial_price"_a, "knock_in_price"_a,
             "knock_out_prices"_a, "coupon_barriers"_a, "upper_strike"_a,
             "lower_strike"_a, "observations"_a, "frequency"_a,
             "touch_status"_a = barrier_touch_status::none, "principal_ratio"_a = 1.0,
             "effective"_a, "expiry"_a)
        .def_prop_ro("coupon_rate", &PhoenixOption::coupon_rate)
        .def_prop_ro("coupon_barriers", &PhoenixOption::coupon_barriers)
        .def("with_coupon_rate", [](const PhoenixOption& option, PythonReal coupon) {
            return unwrap(option.with_coupon_rate(real_number(coupon, "coupon")));
        }, "coupon"_a);
    bind_knock_in_properties(phoenix);

    bind_presets(module);
}

} // namespace kiyosi::python_binding
