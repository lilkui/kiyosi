#include <kiyosi/pricing/engines/autocallable/monte_carlo.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <optional>
#include <random>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/market/schedule.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/autocallable_pricer.hpp"
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

Result<std::array<double, 2>> coupon_path_payoffs(
    double initial_spot, const AutocallableProgram& program,
    const SimulationInputs& inputs, AutocallablePathState state,
    Pcg32& generator, const AutocallableProgram& alternate_program,
    AutocallablePathState alternate_state)
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
        auto event = step.event;
        event.coupon = step.alternate_coupon;
        const double alternate_coupon = program_observation_coupon(event, value);
        if (value >= step.event.knock_out_level) {
            const double payoff = step.simulation.discount.apply(program.principal_ratio + coupon) + state.coupons;
            const double alternate_payoff = step.simulation.discount.apply(alternate_program.principal_ratio + alternate_coupon) + alternate_state.coupons;
            return std::array{payoff, alternate_payoff};
        }
        if (program.carries_observation_coupon)
            state.coupons += step.simulation.discount.apply(coupon);
        if (alternate_program.carries_observation_coupon)
            alternate_state.coupons += step.simulation.discount.apply(alternate_coupon);
    }
    state.knocked_in = program_knocked_in(program, value, state.knocked_in, true);
    const double payoff = state.coupons + inputs.terminal_discount.apply(program_terminal_settlement(program, value, state.knocked_in));
    const double alternate_payoff = alternate_state.coupons + inputs.terminal_discount.apply(program_terminal_settlement(alternate_program, value, state.knocked_in));
    return std::array{payoff, alternate_payoff};
}

struct PathState {
    double spot;
    AutocallablePathState path;
    std::optional<double> payoff;
};

template <std::size_t count, typename Inputs>
Result<std::array<double, count>> path_payoffs(
    const AutocallableProgram& program, const Inputs& inputs,
    const std::array<PathState, count>& initial_states, Pcg32& generator)
{
    auto states = initial_states;
    constexpr bool shared_inputs = std::same_as<Inputs, SimulationInputs>;
    const auto& common = [&]() -> const SimulationInputs& {
        if constexpr (shared_inputs) return inputs;
        else return *std::ranges::max_element(inputs, {}, [](const auto& value) { return value.step_count.size(); });
    }();
    std::normal_distribution<double> normal;
    std::size_t index = 0;
    for (const auto& common_step : common.step_count) {
        const double draw = normal(generator);
        double shared_growth = 0.0;
        if constexpr (shared_inputs)
            shared_growth = std::exp(common_step.simulation.drift + common_step.simulation.diffusion * draw);
        for (std::size_t i = 0; i < count; ++i) {
            auto& state = states[i];
            if constexpr (count > 1)
                if (state.payoff) continue;
            const auto& step = [&]() -> const CudaAutocallableStep& {
                if constexpr (shared_inputs) return common_step;
                else return inputs[i].step_count[index];
            }();
            const double growth = [&] {
                if constexpr (shared_inputs) return shared_growth;
                else return std::exp(step.simulation.drift + step.simulation.diffusion * draw);
            }();
            state.spot *= growth;
            if (!std::isfinite(state.spot) || state.spot <= 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "Monte Carlo simulation produced an invalid asset price"});
            state.path.knocked_in = program_knocked_in(program, state.spot, state.path.knocked_in, false);
            if (!step.event.active) continue;
            const double coupon = program_observation_coupon(step.event, state.spot);
            if (state.spot >= step.event.knock_out_level) {
                const double payoff = step.simulation.discount.apply(program.principal_ratio + coupon) + state.path.coupons;
                if constexpr (count == 1) return std::array{payoff};
                else state.payoff = payoff;
            }
            else if (program.carries_observation_coupon)
                state.path.coupons += step.simulation.discount.apply(coupon);
        }
        if constexpr (count > 1)
            if (std::ranges::all_of(states, [](const auto& state) { return state.payoff.has_value(); })) break;
        ++index;
    }
    std::array<double, count> payoffs{};
    for (std::size_t i = 0; i < count; ++i) {
        auto& state = states[i];
        state.path.knocked_in = program_knocked_in(program, state.spot, state.path.knocked_in, true);
        const auto& terminal_discount = [&]() -> const MonteCarloDiscount& {
            if constexpr (shared_inputs) return common.terminal_discount;
            else return inputs[i].terminal_discount;
        }();
        if constexpr (count == 1)
            payoffs[i] = state.path.coupons + terminal_discount.apply(program_terminal_settlement(program, state.spot, state.path.knocked_in));
        else
            payoffs[i] = state.payoff ? *state.payoff : state.path.coupons + terminal_discount.apply(program_terminal_settlement(program, state.spot, state.path.knocked_in));
    }
    return payoffs;
}

template <std::size_t count, typename Note>
Result<std::array<double, count>> price_spot_bumps(
    const Note& note, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double h, [[maybe_unused]] CudaWorkspace* workspace = nullptr)
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
    std::array<double, count> prices{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto inputs = prepare_simulation(note, context, base.next_observation,
                                               program_knock_in_affects_payoff(program) && program.daily_knock_in && !base.path.knocked_in);
        if (!inputs) return std::unexpected(inputs.error());
        if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
            std::array<CudaAutocallableRequest, count> requests{};
            std::array<std::size_t, count> indices{};
            std::size_t active = 0;
            for (std::size_t i = 0; i < count; ++i) {
                if (initial[i].payoff) continue;
                indices[active] = i;
                requests[active++] = {settings.path_count, *settings.seed, initial[i].spot,
                                      inputs->terminal_discount, program, initial[i].path};
            }
            const auto result = cuda_autocallable_spot_prices(std::span{requests}.first(active), inputs->step_count, workspace);
            const auto valid = cuda_mean(result.result);
            if (!valid) return std::unexpected(valid.error());
            for (std::size_t i = 0; i < active; ++i)
                prices[indices[i]] = result.payoff_means[i];
#else
            return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
        } else {
            std::array<MonteCarloMean, count> means{};
            std::mt19937_64 generator{*settings.seed};
            for (int path = 0; path < settings.path_count; ++path) {
                // Each path keeps its scalar stream even when spot bumps knock out on different dates.
                Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
                const auto payoffs = path_payoffs(program, *inputs, initial, path_generator);
                if (!payoffs) return std::unexpected(payoffs.error());
                for (std::size_t i = 0; i < count; ++i)
                    if (!initial[i].payoff) means[i].add((*payoffs)[i]);
            }
            for (std::size_t i = 0; i < count; ++i)
                prices[i] = means[i].value();
        }
    }
    for (std::size_t i = 0; i < count; ++i) {
        const auto value = checked_price(initial[i].payoff ? *initial[i].payoff : prices[i]);
        if (!value) return std::unexpected(value.error());
        prices[i] = *value;
    }
    return prices;
}

template <typename Note>
Result<std::array<double, 3>> price_rate_bumps(
    const Note& note, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double h, std::optional<double> base_price = std::nullopt)
{
    const auto program = autocallable_program(note);
    std::array<SimulationInputs, 3> inputs{};
    std::array<PathState, 3> initial{};
    initial[0].payoff = base_price;
    for (std::size_t i = base_price ? 1 : 0; i < initial.size(); ++i) {
        const double rate = context.model_parameters().risk_free_rate() + std::array{0.0, h, -h}[i];
        const auto shifted = shifted_context(context, context.spot_price(), context.model_parameters().volatility(),
                                             rate, context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        const auto state = autocallable_initial_state(note, *shifted, program);
        const auto fixed = state.settlement ? state.settlement : autocallable_fixed_value(note, *shifted, program, state);
        if (fixed) {
            const auto value = checked_price(*fixed);
            if (!value) return std::unexpected(value.error());
        } else {
            auto prepared = prepare_simulation(note, *shifted, state.next_observation,
                                               program_knock_in_affects_payoff(program) && program.daily_knock_in && !state.path.knocked_in);
            if (!prepared) return std::unexpected(prepared.error());
            inputs[i] = std::move(*prepared);
        }
        initial[i] = {shifted->spot_price(), state.path, fixed};
    }
    std::array<MonteCarloMean, 3> means{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        std::mt19937_64 generator{*settings.seed};
        for (int path = 0; path < settings.path_count; ++path) {
            Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
            const auto payoffs = path_payoffs(program, inputs, initial, path_generator);
            if (!payoffs) return std::unexpected(payoffs.error());
            for (std::size_t i = 0; i < means.size(); ++i)
                if (!initial[i].payoff) means[i].add((*payoffs)[i]);
        }
    }
    std::array<double, 3> prices{};
    for (std::size_t i = 0; i < prices.size(); ++i) {
        const auto value = checked_price(initial[i].payoff ? *initial[i].payoff : means[i].value());
        if (!value) return std::unexpected(value.error());
        prices[i] = *value;
    }
    return prices;
}

template <typename Note>
Result<void> validate_pricing(
    const Note& note, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    bool& contract_validated)
{
    if (!contract_validated) {
        const auto note_validation = validate_autocallable_note(note);
        if (!note_validation) return std::unexpected(note_validation.error());
    }
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), note.effective_date(), note.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (!contract_validated) {
        const auto schedule = validate_observation_trading_days(note.observation_dates(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
        contract_validated = true;
    }
    auto history = validate_autocallable_history(note, context);
    if (!history) return std::unexpected(history.error());
    const auto settings_valid = validate_monte_carlo_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    auto expiry_valid = validate_trading_expiry(context.calendar(), note.expiry_date());
    if (!expiry_valid) return std::unexpected(expiry_valid.error());

    return {};
}

template <typename Note>
Result<double> price_autocallable(
    const Note& note, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings, bool& contract_validated,
    [[maybe_unused]] CudaWorkspace* workspace = nullptr)
{
    const auto valid = validate_pricing(note, context, settings, contract_validated);
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
        const auto payoff = path_payoffs(program, *inputs, std::array{PathState{context.spot_price(), initial.path, {}}}, generator);
        if (!payoff) return std::unexpected(payoff.error());
        return (*payoff)[0];
    });
}

} // namespace

template <typename Note>
Result<void> detail::validate_monte_carlo_autocallable(
    const Note& note, const PricingContext& context, const TradingDayMonteCarloSettings& settings)
{
    bool contract_validated = false;
    return validate_pricing(note, context, settings, contract_validated);
}

template <typename Note>
Result<double> MonteCarloAutocallableEngine<Note>::price(const Note& note, const PricingContext& context) const
{
    bool contract_validated = false;
    return price_autocallable(note, context, settings_, contract_validated);
}

template <typename Note>
Result<double> implied_volatility(
    const MonteCarloAutocallableEngine<Note>& engine, const Note& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    CudaWorkspace* workspace = nullptr;
#if KIYOSI_HAS_CUDA
    CudaWorkspace storage;
    workspace = &storage;
#endif
    // The solver validates the contract and calendar before invoking any trial.
    bool contract_validated = true;
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& seeded, const auto& instrument, const auto& shifted) {
                                                return price_autocallable(instrument, shifted, seeded.settings(), contract_validated, workspace);
                                            });
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
        const auto payoff = coupon_path_payoffs(context.spot_price(), program, *inputs, initial.path,
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
    return detail::price_with_greeks(
        *this, option, context, greeks, settings,
        [&](const auto& seeded) -> Result<PricingResult> {
            bool contract_validated = false;
            const auto calculate = [&](const auto& scalar, CudaWorkspace* workspace) -> Result<PricingResult> {
                return complete_monte_carlo_greeks(scalar, option, context, greeks, settings, seeded.settings().backend,
                    [&](const PricingContext& shifted) { return validate_pricing(option, shifted, seeded.settings(), contract_validated); },
                    [&]<std::size_t count>(const PricingContext& shifted, double shift) {
                        return price_spot_bumps<count>(option, shifted, seeded.settings(), shift, workspace);
                    },
                    [&](double shift, std::optional<double> base_price) {
                        return price_rate_bumps(option, context, seeded.settings(), shift, base_price);
                    });
            };
#if KIYOSI_HAS_CUDA
            if (seeded.settings().backend == MonteCarloBackend::cuda) {
                const CudaMonteCarloPricer prices{[&](const Note& instrument, const PricingContext& shifted, CudaWorkspace* workspace) {
                    return price_autocallable(instrument, shifted, seeded.settings(), contract_validated, workspace);
                }};
                return calculate(prices, &prices.workspace);
            }
#endif
            const AutocallablePricer prices{[&](const Note& instrument, const PricingContext& shifted) {
                return price_autocallable(instrument, shifted, seeded.settings(), contract_validated);
            }};
            return calculate(prices, nullptr);
        },
        true);
}

template KIYOSI_EXPORT Result<void> detail::validate_monte_carlo_autocallable(
    const PhoenixOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<void> detail::validate_monte_carlo_autocallable(
    const SnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<void> detail::validate_monte_carlo_autocallable(
    const BinarySnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<void> detail::validate_monte_carlo_autocallable(
    const TernarySnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);

template KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine<PhoenixOption>&, const PhoenixOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine<SnowballOption>&, const SnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine<BinarySnowballOption>&, const BinarySnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine<TernarySnowballOption>&, const TernarySnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);

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
