#include "_binding.hpp"

#include <string>

using namespace nb::literals;

namespace kiyosi::python_binding {

namespace {

using PythonObservationSequence = nb::typed<nb::handle, nb::typed<nb::iterable, AutocallableObservation>>;

std::vector<AutocallableObservation> observation_sequence(nb::handle values)
{
    std::vector<AutocallableObservation> records;
    for (nb::handle item : nb::borrow<nb::iterable>(values)) {
        if (!nb::isinstance<AutocallableObservation>(item))
            type_error("observations", "an iterable of AutocallableObservation records");
        records.push_back(nb::cast<AutocallableObservation>(item));
    }
    return records;
}

template <typename Note>
void bind_note_properties(nb::class_<Note>& binding)
{
    binding.def_prop_ro("observations", [](const Note& note) {
        const auto records = note.observations_view();
        return std::vector<AutocallableObservation>{records.begin(), records.end()}; }, "Owning copy of the complete observation records.");
    binding.def_prop_ro("knock_out_levels", &Note::knock_out_levels, "Knock-out level for each observation date.").def_prop_ro("observation_dates", [](const Note& note) { return python_dates(note.observations_view() | std::views::transform(&AutocallableObservation::date)); }, "Copy of the ordered observation dates.").def_prop_ro("principal_ratio", &Note::principal_ratio, "Normalized principal repayment component; coupons and downside are separate.").def_prop_ro("barrier_state", &Note::barrier_state, "State before valuation, or None when not supplied.").def_prop_ro("effective_date", [](const Note& note) { return python_date(note.effective_date()); }, "First date on which the note is effective.").def_prop_ro("expiry_date", [](const Note& note) { return python_date(note.expiry_date()); }, "Note expiry date.");
    if constexpr (requires { &Note::initial_spot; &Note::upper_strike; &Note::lower_strike; })
        binding.def_prop_ro("initial_spot", &Note::initial_spot,
                            "Reference spot used to define relative terms.")
            .def_prop_ro("upper_strike", &Note::upper_strike,
                         "Upper terminal participation strike.")
            .def_prop_ro("lower_strike", &Note::lower_strike,
                         "Lower terminal participation strike.");
}

template <typename Note>
void bind_knock_in_properties(nb::class_<Note>& binding)
{
    bind_note_properties(binding);
    binding.def_prop_ro("knock_in_level", &Note::knock_in_level,
                        "Lower knock-in barrier level.")
        .def_prop_ro("knock_in_observation_mode", &Note::knock_in_observation_mode,
                     "Trading-day or expiry-only knock-in observation rule.");
}

std::string snowball_preset_doc(
    const char* parameters, const char* result = "Validated immutable snowball option.")
{
    return std::string{parameters} + R"doc(observation_dates : iterable[datetime.date]
    Ordered knock-out observations at 00:00 UTC.
effective_date, expiry_date : datetime.date
    Note effective and expiry dates, each anchored at 00:00 UTC.
barrier_state : {'none', 'knocked_out', 'knocked_in'} | None, optional
    State before valuation; required after an observation has occurred.
principal_ratio : float, optional
    Normalized principal repayment component; coupons and downside are separate.

Returns
-------
SnowballOption
    )doc" + result +
           R"doc(

Raises
------
TypeError
    If an argument has an incompatible representation.
KiyosiError
    If the core rejects the rates, levels, schedule, state, or dates.)doc";
}

template <typename Terms, typename Factory>
void bind_basic_preset(nb::module_& module, const char* name, Factory factory)
{
    module.def(
        name,
        [factory](PythonReal coupon_rate, PythonReal initial_spot,
                  PythonReal knock_in_level, PythonReal knock_out_level,
                  PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
                  std::optional<PythonChoice<AutocallableBarrierState>> barrier_state, PythonReal principal_ratio) {
            return unwrap(factory(Terms{
                .coupon_rate = real_number(coupon_rate, "coupon_rate"),
                .initial_spot = real_number(initial_spot, "initial_spot"),
                .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                .observation_dates = date_sequence(observation_dates, "observation_dates"),
                .effective_date = calendar_date(effective_date, "effective_date"),
                .expiry_date = calendar_date(expiry_date, "expiry_date"),
                .barrier_state = barrier_state,
                .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "knock_out_level"_a, "observation_dates"_a, "effective_date"_a, "expiry_date"_a,
        "barrier_state"_a = SnowballTerms{}.barrier_state,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        snowball_preset_doc(R"doc(Create a standard or European snowball preset.

The called factory determines whether the knock-in barrier is observed every
trading day or only at expiry.

Parameters
----------
coupon_rate : float
    Annualized coupon rate used for knock-out and maturity coupons.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
knock_out_level : float
    Knock-out level applied to every observation.
)doc")
            .c_str());
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
           std::optional<PythonChoice<AutocallableBarrierState>> barrier_state, PythonReal principal_ratio) {
            return unwrap(make_step_down_snowball({.coupon_rate = real_number(coupon_rate, "coupon_rate"),
                                                   .initial_spot = real_number(initial_spot, "initial_spot"),
                                                   .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                                   .initial_knock_out_level = real_number(initial_knock_out_level, "initial_knock_out_level"),
                                                   .knock_out_level_decrement = real_number(knock_out_level_decrement, "knock_out_level_decrement"),
                                                   .observation_dates = date_sequence(observation_dates, "observation_dates"),
                                                   .effective_date = calendar_date(effective_date, "effective_date"),
                                                   .expiry_date = calendar_date(expiry_date, "expiry_date"),
                                                   .barrier_state = barrier_state,
                                                   .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "initial_knock_out_level"_a, "knock_out_level_decrement"_a, "observation_dates"_a, "effective_date"_a,
        "expiry_date"_a, "barrier_state"_a = SnowballTerms{}.barrier_state,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        snowball_preset_doc(R"doc(Create a snowball with linearly decreasing knock-out barriers.

Parameters
----------
coupon_rate : float
    Annualized coupon rate.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
initial_knock_out_level : float
    Knock-out level at the first observation.
knock_out_level_decrement : float
    Amount subtracted from each successive knock-out level.
)doc")
            .c_str());
    module.def(
        "both_down_snowball",
        [](PythonReal initial_coupon_rate, PythonReal coupon_rate_decrement, PythonReal initial_spot,
           PythonReal knock_in_level, PythonReal initial_knock_out_level, PythonReal knock_out_level_decrement,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           std::optional<PythonChoice<AutocallableBarrierState>> barrier_state, PythonReal principal_ratio) {
            return unwrap(make_both_down_snowball({.initial_coupon_rate = real_number(initial_coupon_rate, "initial_coupon_rate"),
                                                   .coupon_rate_decrement = real_number(coupon_rate_decrement, "coupon_rate_decrement"),
                                                   .initial_spot = real_number(initial_spot, "initial_spot"),
                                                   .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                                   .initial_knock_out_level = real_number(initial_knock_out_level, "initial_knock_out_level"),
                                                   .knock_out_level_decrement = real_number(knock_out_level_decrement, "knock_out_level_decrement"),
                                                   .observation_dates = date_sequence(observation_dates, "observation_dates"),
                                                   .effective_date = calendar_date(effective_date, "effective_date"),
                                                   .expiry_date = calendar_date(expiry_date, "expiry_date"),
                                                   .barrier_state = barrier_state,
                                                   .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "initial_coupon_rate"_a, "coupon_rate_decrement"_a, "initial_spot"_a,
        "knock_in_level"_a, "initial_knock_out_level"_a, "knock_out_level_decrement"_a,
        "observation_dates"_a, "effective_date"_a, "expiry_date"_a,
        "barrier_state"_a = SnowballTerms{}.barrier_state,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        snowball_preset_doc(R"doc(Create a snowball with decreasing coupons and knock-out barriers.

Parameters
----------
initial_coupon_rate : float
    Annualized coupon rate at the first observation.
coupon_rate_decrement : float
    Amount subtracted from each successive coupon rate.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
initial_knock_out_level : float
    Knock-out level at the first observation.
knock_out_level_decrement : float
    Amount subtracted from each successive knock-out level.
)doc")
            .c_str());
    module.def(
        "dual_coupon_snowball",
        [](PythonReal knock_out_coupon_rate, PythonReal maturity_coupon_rate,
           PythonReal initial_spot, PythonReal knock_in_level, PythonReal knock_out_level,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           std::optional<PythonChoice<AutocallableBarrierState>> barrier_state, PythonReal principal_ratio) {
            return unwrap(make_dual_coupon_snowball({.knock_out_coupon_rate = real_number(knock_out_coupon_rate, "knock_out_coupon_rate"),
                                                     .maturity_coupon_rate = real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                                                     .initial_spot = real_number(initial_spot, "initial_spot"),
                                                     .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                                     .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                                                     .observation_dates = date_sequence(observation_dates, "observation_dates"),
                                                     .effective_date = calendar_date(effective_date, "effective_date"),
                                                     .expiry_date = calendar_date(expiry_date, "expiry_date"),
                                                     .barrier_state = barrier_state,
                                                     .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "knock_out_coupon_rate"_a, "maturity_coupon_rate"_a,
        "initial_spot"_a, "knock_in_level"_a, "knock_out_level"_a,
        "observation_dates"_a, "effective_date"_a, "expiry_date"_a,
        "barrier_state"_a = SnowballTerms{}.barrier_state,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        snowball_preset_doc(R"doc(Create a snowball with separate knock-out and maturity coupons.

Parameters
----------
knock_out_coupon_rate : float
    Annualized coupon rate paid after knock-out.
maturity_coupon_rate : float
    Annualized coupon rate paid at maturity when applicable.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
knock_out_level : float
    Knock-out level applied to every observation.
)doc")
            .c_str());
    module.def(
        "parachute_snowball",
        [](PythonReal coupon_rate, PythonReal initial_spot, PythonReal knock_in_level,
           PythonReal knock_out_level, PythonReal final_knock_out_level,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           std::optional<PythonChoice<AutocallableBarrierState>> barrier_state, PythonReal principal_ratio) {
            return unwrap(make_parachute_snowball({.coupon_rate = real_number(coupon_rate, "coupon_rate"),
                                                   .initial_spot = real_number(initial_spot, "initial_spot"),
                                                   .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                                   .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                                                   .final_knock_out_level = real_number(final_knock_out_level, "final_knock_out_level"),
                                                   .observation_dates = date_sequence(observation_dates, "observation_dates"),
                                                   .effective_date = calendar_date(effective_date, "effective_date"),
                                                   .expiry_date = calendar_date(expiry_date, "expiry_date"),
                                                   .barrier_state = barrier_state,
                                                   .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "knock_out_level"_a, "final_knock_out_level"_a, "observation_dates"_a,
        "effective_date"_a, "expiry_date"_a, "barrier_state"_a = SnowballTerms{}.barrier_state,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        snowball_preset_doc(R"doc(Create a snowball with a distinct final knock-out barrier.

Parameters
----------
coupon_rate : float
    Annualized coupon rate.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
knock_out_level : float
    Knock-out level before the final observation.
final_knock_out_level : float
    Knock-out level at the final observation.
)doc")
            .c_str());
    module.def(
        "otm_snowball",
        [](PythonReal coupon_rate, PythonReal initial_spot, PythonReal knock_in_level,
           PythonReal knock_out_level, PythonReal upper_strike,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           std::optional<PythonChoice<AutocallableBarrierState>> barrier_state, PythonReal principal_ratio) {
            return unwrap(make_otm_snowball({.coupon_rate = real_number(coupon_rate, "coupon_rate"),
                                             .initial_spot = real_number(initial_spot, "initial_spot"),
                                             .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                             .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                                             .upper_strike = real_number(upper_strike, "upper_strike"),
                                             .observation_dates = date_sequence(observation_dates, "observation_dates"),
                                             .effective_date = calendar_date(effective_date, "effective_date"),
                                             .expiry_date = calendar_date(expiry_date, "expiry_date"),
                                             .barrier_state = barrier_state,
                                             .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "knock_out_level"_a, "upper_strike"_a, "observation_dates"_a, "effective_date"_a,
        "expiry_date"_a, "barrier_state"_a = SnowballTerms{}.barrier_state,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        snowball_preset_doc(R"doc(Create a snowball with a custom upper terminal strike.

Parameters
----------
coupon_rate : float
    Annualized coupon rate.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
knock_out_level : float
    Knock-out level applied to every observation.
upper_strike : float
    Upper terminal participation strike.
)doc",
                            "Validated immutable out-of-the-money snowball option.")
            .c_str());
    module.def(
        "loss_capped_snowball",
        [](PythonReal coupon_rate, PythonReal initial_spot, PythonReal knock_in_level,
           PythonReal knock_out_level, PythonReal lower_strike,
           PythonDateSequence observation_dates, PythonDate effective_date, PythonDate expiry_date,
           std::optional<PythonChoice<AutocallableBarrierState>> barrier_state, PythonReal principal_ratio) {
            return unwrap(make_loss_capped_snowball({.coupon_rate = real_number(coupon_rate, "coupon_rate"),
                                                     .initial_spot = real_number(initial_spot, "initial_spot"),
                                                     .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                                     .knock_out_level = real_number(knock_out_level, "knock_out_level"),
                                                     .lower_strike = real_number(lower_strike, "lower_strike"),
                                                     .observation_dates = date_sequence(observation_dates, "observation_dates"),
                                                     .effective_date = calendar_date(effective_date, "effective_date"),
                                                     .expiry_date = calendar_date(expiry_date, "expiry_date"),
                                                     .barrier_state = barrier_state,
                                                     .principal_ratio = real_number(principal_ratio, "principal_ratio")}));
        },
        nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
        "knock_out_level"_a, "lower_strike"_a, "observation_dates"_a, "effective_date"_a,
        "expiry_date"_a, "barrier_state"_a = SnowballTerms{}.barrier_state,
        "principal_ratio"_a = SnowballTerms{}.principal_ratio,
        snowball_preset_doc(R"doc(Create a snowball with a lower loss-capping strike.

Parameters
----------
coupon_rate : float
    Annualized coupon rate.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
knock_out_level : float
    Knock-out level applied to every observation.
lower_strike : float
    Lower terminal strike that caps downside loss.
)doc",
                            "Validated immutable loss-capped snowball option.")
            .c_str());
}

} // namespace

void bind_autocallable_instruments(nb::module_& module)
{
    auto observation = nb::class_<AutocallableObservation>(module, "AutocallableObservation",
                                                           "Immutable observation record. Domain validation runs when constructing a note.")
                           .def(nb::new_([](PythonDate date, PythonReal knock_out_level, PythonReal coupon_rate, PythonReal coupon_barrier_level) {
                                    return AutocallableObservation{calendar_date(date, "date"), real_number(knock_out_level, "knock_out_level"),
                                                                   real_number(coupon_rate, "coupon_rate"), real_number(coupon_barrier_level, "coupon_barrier_level")};
                                }),
                                "date"_a, "knock_out_level"_a, nb::kw_only(),
                                "coupon_rate"_a = AutocallableObservation{}.coupon_rate,
                                "coupon_barrier_level"_a = AutocallableObservation{}.coupon_barrier_level)
                           .def_prop_ro("date", [](const AutocallableObservation& value) { return python_date(value.date); })
                           .def_prop_ro("knock_out_level", [](const AutocallableObservation& value) { return value.knock_out_level; })
                           .def_prop_ro("coupon_rate", [](const AutocallableObservation& value) { return value.coupon_rate; })
                           .def_prop_ro("coupon_barrier_level", [](const AutocallableObservation& value) { return value.coupon_barrier_level; });
    bind_value_equality(observation);
    bind_repr(observation, "AutocallableObservation", {"date", "knock_out_level", "coupon_rate", "coupon_barrier_level"});
    module.def(
        "autocallable_observations",
        [](PythonDateSequence observation_dates, PythonRealSequence knock_out_levels,
           PythonRealSequence knock_out_coupon_rates, PythonRealSequence coupon_barrier_levels) {
            auto dates = date_sequence(observation_dates, "observation_dates");
            auto levels = real_sequence(knock_out_levels, "knock_out_levels");
            std::optional<std::vector<double>> coupons, barriers;
            if (!knock_out_coupon_rates.is_none())
                coupons = real_sequence(knock_out_coupon_rates, "knock_out_coupon_rates");
            if (!coupon_barrier_levels.is_none())
                barriers = real_sequence(coupon_barrier_levels, "coupon_barrier_levels");
            return unwrap(make_autocallable_observations(dates, levels, std::move(coupons), std::move(barriers)));
        },
        "observation_dates"_a, "knock_out_levels"_a, nb::kw_only(),
        "knock_out_coupon_rates"_a.none() = nb::none(), "coupon_barrier_levels"_a.none() = nb::none(),
        R"doc(Associate parallel sequences into owning observation records.

Dates, knock-out levels, and supplied coupon sequences must have equal lengths.
Omitted coupon sequences use the core record defaults. Dates and contractual
values are validated when an autocallable note is constructed from the records.

Raises
------
TypeError
    If a sequence or element has an incompatible representation.
KiyosiError
    With category INVALID_SCHEDULE if sequence lengths differ.)doc");
    auto snowball = nb::class_<SnowballOption>(
                        module, "SnowballOption", R"doc(Immutable validated snowball option.)doc")
                        .def(nb::new_([](PythonReal maturity_coupon_rate, PythonReal initial_spot,
                                         PythonReal knock_in_level, PythonReal upper_strike, PythonReal lower_strike,
                                         PythonChoice<KnockInObservationMode> knock_in_observation_mode, std::optional<PythonChoice<AutocallableBarrierState>> barrier_state,
                                         PythonReal principal_ratio, PythonDate effective_date, PythonDate expiry_date, PythonObservationSequence observations) {
                                 SnowballTerms terms{.maturity_coupon_rate = real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                                                     .initial_spot = real_number(initial_spot, "initial_spot"),
                                                     .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                                     .upper_strike = real_number(upper_strike, "upper_strike"),
                                                     .lower_strike = real_number(lower_strike, "lower_strike"),
                                                     .knock_in_observation_mode = knock_in_observation_mode,
                                                     .barrier_state = barrier_state,
                                                     .principal_ratio = real_number(principal_ratio, "principal_ratio"),
                                                     .effective_date = calendar_date(effective_date, "effective_date"),
                                                     .expiry_date = calendar_date(expiry_date, "expiry_date")};
                                 terms.observations = observation_sequence(observations);
                                 return unwrap(make_snowball_option(terms));
                             }),
                             nb::kw_only(), "maturity_coupon_rate"_a,
                             "initial_spot"_a, "knock_in_level"_a, "upper_strike"_a, "lower_strike"_a, "knock_in_observation_mode"_a,
                             "barrier_state"_a = SnowballTerms{}.barrier_state,
                             "principal_ratio"_a = SnowballTerms{}.principal_ratio,
                             "effective_date"_a, "expiry_date"_a, "observations"_a, R"doc(Create a validated snowball option.

Provide observations as AutocallableObservation records. Use autocallable_observations
to associate parallel sequences before construction. Domain validation runs in the core.

Parameters
----------
observations : iterable[AutocallableObservation]
    Ordered contractual observations at 00:00 UTC.
maturity_coupon_rate : float
    Annualized coupon rate used at maturity when applicable.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
upper_strike, lower_strike : float
    Terminal participation strikes.
knock_in_observation_mode : {'every_trading_day', 'at_expiry'}
    Trading-day or expiry-only knock-in observation rule.
barrier_state : {'none', 'knocked_out', 'knocked_in'} | None, optional
    State before valuation; required after an observation has occurred.
principal_ratio : float, optional
    Normalized principal repayment component; coupons and downside are separate.
effective_date, expiry_date : datetime.date
    Note effective and expiry dates, each anchored at 00:00 UTC.

Raises
------
TypeError
    If an argument or sequence element has an incompatible representation.
KiyosiError
    If the core rejects the rates, levels, schedule, state, or dates.)doc")
                        .def_prop_ro("knock_out_coupon_rates", &SnowballOption::knock_out_coupon_rates, "Annualized coupon rate for each observation date.")
                        .def_prop_ro("maturity_coupon_rate", &SnowballOption::maturity_coupon_rate, "Annualized coupon rate used at maturity when applicable.");
    bind_knock_in_properties(snowball);
    bind_value_equality(snowball);
    bind_repr(snowball, "SnowballOption",
              {"observations", "maturity_coupon_rate",
               "initial_spot",
               "knock_in_level",
               "upper_strike",
               "lower_strike",
               "knock_in_observation_mode",
               "barrier_state",
               "principal_ratio",
               "effective_date",
               "expiry_date"});

    auto binary = nb::class_<BinarySnowballOption>(
                      module, "BinarySnowballOption", R"doc(Immutable validated binary snowball option.)doc")
                      .def(nb::new_([](PythonReal maturity_coupon_rate,
                                       std::optional<PythonChoice<AutocallableBarrierState>> barrier_state, PythonReal principal_ratio,
                                       PythonDate effective_date, PythonDate expiry_date, PythonObservationSequence observations) {
                               BinarySnowballTerms terms{.maturity_coupon_rate = real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                                                         .barrier_state = barrier_state,
                                                         .principal_ratio = real_number(principal_ratio, "principal_ratio"),
                                                         .effective_date = calendar_date(effective_date, "effective_date"),
                                                         .expiry_date = calendar_date(expiry_date, "expiry_date")};
                               terms.observations = observation_sequence(observations);
                               return unwrap(make_binary_snowball_option(terms));
                           }),
                           nb::kw_only(), "maturity_coupon_rate"_a,
                           "barrier_state"_a = BinarySnowballTerms{}.barrier_state,
                           "principal_ratio"_a = BinarySnowballTerms{}.principal_ratio,
                           "effective_date"_a, "expiry_date"_a, "observations"_a,
                           R"doc(Create a validated binary snowball option.

Provide observations as AutocallableObservation records. Use autocallable_observations
to associate parallel sequences before construction. Domain validation runs in the core.

Parameters
----------
observations : iterable[AutocallableObservation]
    Ordered contractual observations at 00:00 UTC.
maturity_coupon_rate : float
    Annualized coupon rate used at maturity when applicable.
barrier_state : {'none', 'knocked_out', 'knocked_in'} | None, optional
    State before valuation; required after an observation has occurred.
principal_ratio : float, optional
    Normalized principal repayment component; coupons and downside are separate.
effective_date, expiry_date : datetime.date
    Note effective and expiry dates, each anchored at 00:00 UTC.

Raises
------
TypeError
    If an argument or sequence element has an incompatible representation.
KiyosiError
    If the core rejects the rates, levels, schedule, state, or dates.)doc")
                      .def_prop_ro("knock_out_coupon_rates", &BinarySnowballOption::knock_out_coupon_rates, "Annualized coupon rate for each observation date.")
                      .def_prop_ro("maturity_coupon_rate", &BinarySnowballOption::maturity_coupon_rate, "Annualized coupon rate used at maturity when applicable.");
    bind_note_properties(binary);
    bind_value_equality(binary);
    bind_repr(binary, "BinarySnowballOption",
              {"observations", "maturity_coupon_rate",
               "barrier_state",
               "principal_ratio",
               "effective_date",
               "expiry_date"});

    auto ternary = nb::class_<TernarySnowballOption>(
                       module, "TernarySnowballOption", R"doc(Immutable validated ternary snowball option.)doc")
                       .def(nb::new_([](PythonReal maturity_coupon_rate, PythonReal minimum_coupon_rate,
                                        PythonReal knock_in_level,
                                        PythonChoice<KnockInObservationMode> knock_in_observation_mode, std::optional<PythonChoice<AutocallableBarrierState>> barrier_state,
                                        PythonReal principal_ratio, PythonDate effective_date, PythonDate expiry_date, PythonObservationSequence observations) {
                                TernarySnowballTerms terms{.maturity_coupon_rate = real_number(maturity_coupon_rate, "maturity_coupon_rate"),
                                                           .minimum_coupon_rate = real_number(minimum_coupon_rate, "minimum_coupon_rate"),
                                                           .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                                           .knock_in_observation_mode = knock_in_observation_mode,
                                                           .barrier_state = barrier_state,
                                                           .principal_ratio = real_number(principal_ratio, "principal_ratio"),
                                                           .effective_date = calendar_date(effective_date, "effective_date"),
                                                           .expiry_date = calendar_date(expiry_date, "expiry_date")};
                                terms.observations = observation_sequence(observations);
                                return unwrap(make_ternary_snowball_option(terms));
                            }),
                            nb::kw_only(), "maturity_coupon_rate"_a,
                            "minimum_coupon_rate"_a, "knock_in_level"_a,
                            "knock_in_observation_mode"_a,
                            "barrier_state"_a = TernarySnowballTerms{}.barrier_state,
                            "principal_ratio"_a = TernarySnowballTerms{}.principal_ratio,
                            "effective_date"_a, "expiry_date"_a, "observations"_a,
                            R"doc(Create a validated ternary snowball option.

Provide observations as AutocallableObservation records. Use autocallable_observations
to associate parallel sequences before construction. Domain validation runs in the core.

Parameters
----------
observations : iterable[AutocallableObservation]
    Ordered contractual observations at 00:00 UTC.
maturity_coupon_rate : float
    Annualized coupon rate used at maturity when applicable.
minimum_coupon_rate : float
    Minimum annualized coupon rate for the third payoff region.
knock_in_level : float
    Lower knock-in barrier level.
knock_in_observation_mode : {'every_trading_day', 'at_expiry'}
    Trading-day or expiry-only knock-in observation rule.
barrier_state : {'none', 'knocked_out', 'knocked_in'} | None, optional
    State before valuation; required after an observation has occurred.
principal_ratio : float, optional
    Normalized principal repayment component; coupons and downside are separate.
effective_date, expiry_date : datetime.date
    Note effective and expiry dates, each anchored at 00:00 UTC.

Raises
------
TypeError
    If an argument or sequence element has an incompatible representation.
KiyosiError
    If the core rejects the rates, levels, schedule, state, or dates.)doc")
                       .def_prop_ro("knock_out_coupon_rates", &TernarySnowballOption::knock_out_coupon_rates, "Annualized coupon rate for each observation date.")
                       .def_prop_ro("maturity_coupon_rate", &TernarySnowballOption::maturity_coupon_rate, "Annualized coupon rate used at maturity when applicable.")
                       .def_prop_ro("minimum_coupon_rate", &TernarySnowballOption::minimum_coupon_rate, "Minimum annualized coupon rate for the third payoff region.");
    bind_knock_in_properties(ternary);
    bind_value_equality(ternary);
    bind_repr(ternary, "TernarySnowballOption",
              {"observations", "maturity_coupon_rate",
               "minimum_coupon_rate",
               "knock_in_level",
               "knock_in_observation_mode",
               "barrier_state",
               "principal_ratio",
               "effective_date",
               "expiry_date"});

    auto phoenix = nb::class_<PhoenixOption>(
                       module, "PhoenixOption", R"doc(Immutable validated Phoenix autocallable option.)doc")
                       .def(nb::new_([](PythonReal coupon_rate, PythonReal initial_spot,
                                        PythonReal knock_in_level, PythonReal upper_strike,
                                        PythonReal lower_strike, PythonChoice<KnockInObservationMode> knock_in_observation_mode, std::optional<PythonChoice<AutocallableBarrierState>> barrier_state,
                                        PythonReal principal_ratio, PythonDate effective_date, PythonDate expiry_date, PythonObservationSequence observations) {
                                PhoenixTerms terms{.coupon_rate = real_number(coupon_rate, "coupon_rate"),
                                                   .initial_spot = real_number(initial_spot, "initial_spot"),
                                                   .knock_in_level = real_number(knock_in_level, "knock_in_level"),
                                                   .upper_strike = real_number(upper_strike, "upper_strike"),
                                                   .lower_strike = real_number(lower_strike, "lower_strike"),
                                                   .knock_in_observation_mode = knock_in_observation_mode,
                                                   .barrier_state = barrier_state,
                                                   .principal_ratio = real_number(principal_ratio, "principal_ratio"),
                                                   .effective_date = calendar_date(effective_date, "effective_date"),
                                                   .expiry_date = calendar_date(expiry_date, "expiry_date")};
                                terms.observations = observation_sequence(observations);
                                return unwrap(make_phoenix_option(terms));
                            }),
                            nb::kw_only(), "coupon_rate"_a, "initial_spot"_a, "knock_in_level"_a,
                            "upper_strike"_a,
                            "lower_strike"_a, "knock_in_observation_mode"_a,
                            "barrier_state"_a = PhoenixTerms{}.barrier_state,
                            "principal_ratio"_a = PhoenixTerms{}.principal_ratio,
                            "effective_date"_a, "expiry_date"_a, "observations"_a, R"doc(Create a validated Phoenix option.

Parameters
----------
observations : iterable[AutocallableObservation]
    Ordered contractual observations at 00:00 UTC.
coupon_rate : float
    Annualized coupon rate, accrued from the previous observation (the effective
    date for the first coupon) using Actual/365 Fixed, in principal-ratio units.
initial_spot : float
    Positive reference spot.
knock_in_level : float
    Lower knock-in barrier level.
upper_strike, lower_strike : float
    Terminal participation strikes.
knock_in_observation_mode : {'every_trading_day', 'at_expiry'}
    Trading-day or expiry-only knock-in observation rule.
barrier_state : {'none', 'knocked_out', 'knocked_in'} | None, optional
    State before valuation; required after an observation has occurred.
principal_ratio : float, optional
    Principal repayment component in normalized payoff units; coupons are separate.
effective_date, expiry_date : datetime.date
    Note effective and expiry dates, each anchored at 00:00 UTC.

Raises
------
TypeError
    If an argument or sequence element has an incompatible representation.
KiyosiError
    If the core rejects the rates, levels, schedule, state, or dates.)doc")
                       .def_prop_ro("coupon_rate", &PhoenixOption::coupon_rate,
                                    "Annualized coupon rate per observation period (Actual/365 Fixed).")
                       .def_prop_ro("coupon_barrier_levels", &PhoenixOption::coupon_barrier_levels, "Coupon barrier level for each observation date.");
    bind_knock_in_properties(phoenix);
    bind_value_equality(phoenix);
    bind_repr(phoenix, "PhoenixOption",
              {"observations", "coupon_rate", "initial_spot", "knock_in_level", "upper_strike", "lower_strike", "knock_in_observation_mode", "barrier_state", "principal_ratio", "effective_date", "expiry_date"});

    bind_presets(module);
}

} // namespace kiyosi::python_binding
