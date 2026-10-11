#include <kiyosi/pricing/engines/autocallable/monte_carlo.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <optional>
#include <random>
#include <utility>
#include <vector>

#include <kiyosi/market/schedule.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../monte_carlo_host.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct SimulationInputs {
    std::vector<CudaAutocallableStep> step_count;
    MonteCarloDiscount terminal_discount;
};

template <typename Note>
Result<SimulationInputs> prepare_simulation(
    const Note& note, const PricingContext& context, std::size_t next_observation, bool daily_knock_in,
    const Note* alternate = nullptr)
{
    const double rate = context.model_parameters().risk_free_rate();
    const Timestamp valuation = context.valuation_time();
    const auto dates = [&] {
        if (daily_knock_in) return trading_dates(context.calendar(), valuation, note.expiry_date());
        std::vector<Date> events(note.observation_dates().begin() + next_observation, note.observation_dates().end());
        if (events.empty() || events.back() != note.expiry_date()) events.push_back(note.expiry_date());
        return events;
    }();
    std::vector<CudaAutocallableStep> steps;
    steps.reserve(dates.size());
    auto previous = valuation;
    for (const Date current : dates) {
        AutocallableEvent event{};
        double alternate_coupon = 0.0;
        if (next_observation < note.observation_dates().size() &&
            note.observation_dates()[next_observation] == current) {
            event = autocallable_event(note, next_observation);
            if (alternate) alternate_coupon = autocallable_event(*alternate, next_observation).coupon;
            ++next_observation;
        }
        const auto step = simulation_step(context, previous, current);
        if (!step) return std::unexpected(step.error());
        steps.push_back({*step, event, alternate_coupon});
        previous = current;
    }
    const double exponent = -rate * actual_365_fixed_year_fraction(valuation, note.expiry_date());
    return SimulationInputs{std::move(steps), {std::exp(exponent), exponent}};
}

template <bool Paired = false>
Result<std::array<double, 2>> path_payoff(double initial_spot, const AutocallableProgram& program,
                                          const SimulationInputs& inputs, AutocallablePathState state,
                                          Pcg32& generator, const AutocallableProgram& alternate_program = {},
                                          AutocallablePathState alternate_state = {})
{
    double value = initial_spot;
    std::normal_distribution<double> normal;
    for (const auto& step : inputs.step_count) {
        value *= std::exp(step.simulation.drift +
                          step.simulation.diffusion * normal(generator));
        if (!std::isfinite(value) || value <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "Monte Carlo simulation produced an invalid asset price"});
        state.knocked_in = program_knocked_in(program, value, state.knocked_in, false);
        if (!step.event.active) continue;
        const double coupon = program_observation_coupon(step.event, value);
        double alternate_coupon = 0.0;
        if constexpr (Paired) {
            auto event = step.event;
            event.coupon = step.alternate_coupon;
            alternate_coupon = program_observation_coupon(event, value);
        }
        if (value >= step.event.knock_out_level) {
            const double payoff = step.simulation.discount.apply(program.principal_ratio + coupon) + state.coupons;
            double alternate_payoff = 0.0;
            if constexpr (Paired)
                alternate_payoff = step.simulation.discount.apply(alternate_program.principal_ratio + alternate_coupon) + alternate_state.coupons;
            return std::array{payoff, alternate_payoff};
        }
        if (program.carries_observation_coupon)
            state.coupons += step.simulation.discount.apply(coupon);
        if constexpr (Paired)
            if (alternate_program.carries_observation_coupon)
                alternate_state.coupons += step.simulation.discount.apply(alternate_coupon);
    }
    state.knocked_in = program_knocked_in(program, value, state.knocked_in, true);
    const double payoff = state.coupons + inputs.terminal_discount.apply(program_terminal_settlement(program, value, state.knocked_in));
    double alternate_payoff = 0.0;
    if constexpr (Paired)
        alternate_payoff = alternate_state.coupons + inputs.terminal_discount.apply(program_terminal_settlement(alternate_program, value, state.knocked_in));
    return std::array{payoff, alternate_payoff};
}

struct PathState {
    double spot;
    AutocallablePathState path;
    std::optional<double> payoff;
};

template <std::size_t count>
Result<std::array<double, count>> path_payoffs(
    const AutocallableProgram& program, const SimulationInputs& inputs,
    std::array<PathState, count> states, Pcg32& generator)
{
    std::normal_distribution<double> normal;
    for (const auto& step : inputs.step_count) {
        const double growth = std::exp(step.simulation.drift + step.simulation.diffusion * normal(generator));
        for (auto& state : states) {
            if (state.payoff) continue;
            state.spot *= growth;
            if (!std::isfinite(state.spot) || state.spot <= 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "Monte Carlo simulation produced an invalid asset price"});
            state.path.knocked_in = program_knocked_in(program, state.spot, state.path.knocked_in, false);
            if (!step.event.active) continue;
            const double coupon = program_observation_coupon(step.event, state.spot);
            if (state.spot >= step.event.knock_out_level)
                state.payoff = step.simulation.discount.apply(program.principal_ratio + coupon) + state.path.coupons;
            else if (program.carries_observation_coupon)
                state.path.coupons += step.simulation.discount.apply(coupon);
        }
        if (std::ranges::all_of(states, [](const auto& state) { return state.payoff.has_value(); })) break;
    }
    std::array<double, count> payoffs{};
    for (std::size_t i = 0; i < count; ++i) {
        auto& state = states[i];
        state.path.knocked_in = program_knocked_in(program, state.spot, state.path.knocked_in, true);
        payoffs[i] = state.payoff ? *state.payoff : state.path.coupons + inputs.terminal_discount.apply(program_terminal_settlement(program, state.spot, state.path.knocked_in));
    }
    return payoffs;
}

template <std::size_t count, typename Note>
Result<std::array<double, count>> price_spot_bumps(
    const Note& note, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double h)
{
    constexpr std::array offsets = count == 2 ? std::array{1.0, -1.0, 0.0, 0.0, 0.0} : std::array{0.0, 1.0, -1.0, 2.0, -2.0};
    const auto program = autocallable_program(note);
    const auto base = autocallable_initial_state(note, context, program);
    std::array<PathState, count> initial{};
    for (std::size_t i = 0; i < count; ++i) {
        const double spot = context.spot_price() + offsets[i] * h;
        const auto shifted = shifted_context(context, spot, context.model_parameters().volatility(),
                                             context.model_parameters().risk_free_rate(), context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        const auto state = autocallable_initial_state(note, *shifted, program);
        const auto fixed = state.settlement ? state.settlement : autocallable_fixed_value(note, *shifted, program, state);
        if (fixed) {
            const auto value = checked_price(*fixed);
            if (!value) return std::unexpected(value.error());
        }
        initial[i] = {spot, state.path, fixed};
    }
    std::array<MonteCarloMean, count> means{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto inputs = prepare_simulation(note, context, base.next_observation,
                                               program_knock_in_affects_payoff(program) && program.daily_knock_in && !base.path.knocked_in);
        if (!inputs) return std::unexpected(inputs.error());
        std::mt19937_64 generator{*settings.seed};
        for (int path = 0; path < settings.path_count; ++path) {
            // Each path keeps its scalar stream even when spot bumps knock out on different dates.
            Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
            const auto payoffs = path_payoffs(program, *inputs, initial, path_generator);
            if (!payoffs) return std::unexpected(payoffs.error());
            for (std::size_t i = 0; i < count; ++i)
                if (!initial[i].payoff) means[i].add((*payoffs)[i]);
        }
    }
    std::array<double, count> prices{};
    for (std::size_t i = 0; i < count; ++i) {
        const auto value = checked_price(initial[i].payoff ? *initial[i].payoff : means[i].value());
        if (!value) return std::unexpected(value.error());
        prices[i] = *value;
    }
    return prices;
}

} // namespace

template <typename Note>
Result<void> detail::validate_monte_carlo_autocallable(
    const Note& note, const PricingContext& context, const TradingDayMonteCarloSettings& settings)
{
    auto note_validation = validate_autocallable_note(note);
    if (!note_validation) return std::unexpected(note_validation.error());
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), note.effective_date(), note.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    auto schedule = validate_observation_trading_days(note.observation_dates(), context.calendar());
    if (!schedule) return std::unexpected(schedule.error());
    auto history = validate_autocallable_history(note, context);
    if (!history) return std::unexpected(history.error());
    const auto settings_valid = validate_monte_carlo_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    auto expiry_valid = validate_trading_expiry(context.calendar(), note.expiry_date());
    if (!expiry_valid) return std::unexpected(expiry_valid.error());

    return {};
}

namespace {
template <typename Note>
Result<double> price_autocallable(
    const Note& note, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings, [[maybe_unused]] CudaWorkspace* workspace = nullptr)
{
    const auto valid = validate_monte_carlo_autocallable(note, context, settings);
    if (!valid) return std::unexpected(valid.error());

    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);
    if (initial.settlement) return checked_price(*initial.settlement);
    if (const auto fixed = autocallable_fixed_value(note, context, program, initial))
        return checked_price(*fixed);

    const auto inputs = prepare_simulation(note, context, initial.next_observation,
                                           program_knock_in_affects_payoff(program) && program.daily_knock_in && !initial.path.knocked_in);
    if (!inputs) return std::unexpected(inputs.error());
    if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto mean = cuda_mean(cuda_autocallable_price(
            {settings.path_count, settings.seed ? *settings.seed : random_seed(),
             context.spot_price(), inputs->terminal_discount, program, initial.path},
            inputs->step_count, workspace));
        if (!mean) return std::unexpected(mean.error());
        return checked_price(*mean);
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }

    return cpu_path_mean(settings, [&](auto& generator) -> Result<double> {
        const auto payoff = path_payoff(context.spot_price(), program, *inputs, initial.path, generator);
        if (!payoff) return std::unexpected(payoff.error());
        return (*payoff)[0];
    });
}
} // namespace

template <typename Note>
Result<double> MonteCarloAutocallableEngine<Note>::price(const Note& note, const PricingContext& context) const
{
    return price_autocallable(note, context, settings_);
}

template <typename Note>
Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const Note& lower, const Note& upper, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings)
{
    for (const auto* note : {&lower, &upper}) {
        const auto valid = validate_monte_carlo_autocallable(*note, context, settings);
        if (!valid) return std::unexpected(valid.error());
    }
    const auto program = autocallable_program(lower);
    const auto alternate_program = autocallable_program(upper);
    const auto initial = autocallable_initial_state(lower, context, program);
    const auto alternate_initial = autocallable_initial_state(upper, context, alternate_program);
    const bool daily = program_knock_in_affects_payoff(program) && program.daily_knock_in && !initial.path.knocked_in;
    const bool alternate_daily = program_knock_in_affects_payoff(alternate_program) && alternate_program.daily_knock_in && !alternate_initial.path.knocked_in;
    // Preserve each endpoint's shortcuts and random time grid when a coupon removes knock-in exposure.
    if (initial.settlement || alternate_initial.settlement ||
        autocallable_fixed_value(lower, context, program, initial) ||
        autocallable_fixed_value(upper, context, alternate_program, alternate_initial) || daily != alternate_daily) {
        const MonteCarloAutocallableEngine<Note> engine{settings};
        const auto first = engine.price(lower, context);
        if (!first) return std::unexpected(first.error());
        const auto second = engine.price(upper, context);
        if (!second) return std::unexpected(second.error());
        return std::array{*first, *second};
    }
    const auto inputs = prepare_simulation(lower, context, initial.next_observation, daily, &upper);
    if (!inputs) return std::unexpected(inputs.error());
    if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto result = cuda_autocallable_coupon_prices(
            {settings.path_count, settings.seed ? *settings.seed : random_seed(), context.spot_price(),
             inputs->terminal_discount, program, initial.path, alternate_program, alternate_initial.path},
            inputs->step_count);
        const auto first = cuda_mean(result);
        if (!first) return std::unexpected(first.error());
        return std::array{*first, result.alternate_payoff_mean};
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
    }
    std::mt19937_64 generator(settings.seed ? *settings.seed : std::random_device{}());
    std::array<MonteCarloMean, 2> means{};
    for (int path = 0; path < settings.path_count; ++path) {
        Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
        const auto payoff = path_payoff<true>(context.spot_price(), program, *inputs, initial.path,
                                              path_generator, alternate_program, alternate_initial.path);
        if (!payoff) return std::unexpected(payoff.error());
        for (std::size_t i = 0; i < means.size(); ++i)
            means[i].add((*payoff)[i]);
    }
    std::array<double, 2> prices{};
    for (std::size_t i = 0; i < prices.size(); ++i) {
        const auto price = checked_price(means[i].value());
        if (!price) return std::unexpected(price.error());
        prices[i] = *price;
    }
    return prices;
}

template <typename Note>
Result<PricingResult> MonteCarloAutocallableEngine<Note>::price_with_greeks(const Note& option, const PricingContext& context,
                                                                            GreeksRequest greeks, NumericalShiftSettings settings) const
{
#if KIYOSI_HAS_CUDA
    if (settings_.backend == MonteCarloBackend::cuda)
        return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& seeded) -> Result<PricingResult> {
            const CudaMonteCarloPricer prices{[&](const Note& instrument, const PricingContext& shifted, CudaWorkspace* workspace) {
                return price_autocallable(instrument, shifted, seeded.settings(), workspace);
            }};
            const auto value = prices.price(option, context);
            if (!value) return std::unexpected(value.error());
            return detail::complete_greeks(prices, option, context, greeks, settings, *make_pricing_result(*value)); }, true);
#endif
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& seeded) -> Result<PricingResult> {
                                         const auto native = [&]() -> Result<PricingResult> {
                                         const auto valid = validate_monte_carlo_autocallable(option, context, seeded.settings());
                                         if (!valid) return std::unexpected(valid.error());
                                         const double spot = context.spot_price();
                                         const double h = symmetric_shift(spot, settings.spot_shift);
                                         const double two_h = 2.0 * h;
                                         const bool speed_available = std::isfinite(two_h) && spot > two_h &&
                                                                      symmetric_shift(spot, two_h) == two_h &&
                                                                      !at_spot_discontinuity(option, context, two_h);
                                         if (seeded.settings().backend == MonteCarloBackend::cpu &&
                                             (greeks.has(Greek::delta) || greeks.has(Greek::gamma) || greeks.has(Greek::speed)) &&
                                             h > 0.0 && spot > h && !greeks_unavailable(option, context) &&
                                             !at_spot_discontinuity(option, context, h) &&
                                             (!greeks.has(Greek::speed) || speed_available)) {
                                             if (greeks.has(Greek::speed))
                                                 return spot_bump_result(price_spot_bumps<5>(option, context, seeded.settings(), h), greeks, h);
                                             return spot_bump_result(price_spot_bumps<3>(option, context, seeded.settings(), h), greeks, h);
                                         }
                                         const auto value = seeded.price(option, context);
                                         if (!value) return std::unexpected(value.error());
                                         return make_pricing_result(*value);
                                         }();
                                         if (!native) return std::unexpected(native.error());
                                         const double h = symmetric_shift(context.spot_price(), settings.spot_shift);
                                         const bool mixed = seeded.settings().backend == MonteCarloBackend::cpu &&
                                                            (greeks.has(Greek::vanna) || greeks.has(Greek::zomma)) &&
                                                            context.spot_price() > h && !at_spot_discontinuity(option, context, h);
                                         const auto sample = [&](const PricingContext& shifted, double shift, bool center) -> Result<std::array<double, 3>> {
                                             const auto valid = validate_monte_carlo_autocallable(option, shifted, seeded.settings());
                                             if (!valid) return std::unexpected(valid.error());
                                             if (center) return price_spot_bumps<3>(option, shifted, seeded.settings(), shift);
                                             const auto values = price_spot_bumps<2>(option, shifted, seeded.settings(), shift);
                                             if (!values) return std::unexpected(values.error());
                                             return std::array{0.0, (*values)[0], (*values)[1]};
                                         };
                                         const MonteCarloMixedPricer prices{seeded, context, mixed ? h : 0.0,
                                                                            symmetric_shift(context.model_parameters().volatility(), settings.volatility_shift),
                                                                            greeks.has(Greek::vega) || greeks.has(Greek::zomma), sample};
                                         return detail::complete_greeks(prices, option, context, greeks, settings, *native); }, true);
}

template KIYOSI_EXPORT Result<void> detail::validate_monte_carlo_autocallable(
    const PhoenixOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<void> detail::validate_monte_carlo_autocallable(
    const SnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<void> detail::validate_monte_carlo_autocallable(
    const BinarySnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<void> detail::validate_monte_carlo_autocallable(
    const TernarySnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);

template class MonteCarloAutocallableEngine<PhoenixOption>;
template class MonteCarloAutocallableEngine<SnowballOption>;
template class MonteCarloAutocallableEngine<BinarySnowballOption>;
template class MonteCarloAutocallableEngine<TernarySnowballOption>;

template KIYOSI_EXPORT Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const PhoenixOption&, const PhoenixOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const SnowballOption&, const SnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const BinarySnowballOption&, const BinarySnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const TernarySnowballOption&, const TernarySnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);

} // namespace kiyosi
