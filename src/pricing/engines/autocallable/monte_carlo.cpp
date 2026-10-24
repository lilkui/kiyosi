#include <kiyosi/pricing/engines/autocallable/monte_carlo.hpp>

#include "../../detail/numerical_greeks.hpp"
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
#include "../monte_carlo_autocallable.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct SimulationInputs {
    std::vector<AutocallableStep> step_count;
    MonteCarloDiscount terminal_discount;
};

struct SimulationView {
    std::span<const AutocallableStep> step_count;
    MonteCarloDiscount terminal_discount;
};

template <AutocallableProduct Note>
Result<SimulationInputs> prepare_simulation(
    const Note& note, const PricingContext& context, std::size_t next_observation, bool daily_knock_in,
    const Note* alternate = nullptr)
{
    const double rate = context.model_parameters().risk_free_rate();
    const Timestamp valuation = context.valuation_time();
    const auto dates = autocallable_event_dates(note, context, next_observation, daily_knock_in);
    std::vector<AutocallableStep> steps;
    steps.reserve(dates.size());
    auto previous = valuation;
    const auto observations = note.observations_view();
    for (const Date current : dates) {
        AutocallableEvent event{};
        double alternate_coupon = 0.0;
        if (next_observation < observations.size() &&
            observations[next_observation].date == current) {
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
        double payoff = 0.0, alternate_payoff = 0.0;
        const bool knocked_out = observe_autocallable_path(program, step.event, step.simulation.discount, value, state, payoff);
        auto event = step.event;
        event.coupon = step.alternate_coupon;
        observe_autocallable_path(alternate_program, event, step.simulation.discount, value, alternate_state, alternate_payoff);
        if (knocked_out) return std::array{payoff, alternate_payoff};
    }
    const double payoff = settle_autocallable_path(program, inputs.terminal_discount, value, state);
    const double alternate_payoff = settle_autocallable_path(alternate_program, inputs.terminal_discount, value, alternate_state);
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
    const auto observe = [&](auto& state, const auto& step) {
        double payoff = 0.0;
        if (observe_autocallable_path(program, step.event, step.simulation.discount, state.spot, state.path, payoff))
            state.payoff = payoff;
    };
    const auto simulated = [&] {
        if constexpr (shared_inputs)
            return simulate_path_batch<true>(inputs.step_count, states, generator, observe);
        else {
            std::array<std::span<const AutocallableStep>, count> sequences{};
            for (std::size_t i = 0; i < count; ++i)
                sequences[i] = inputs[i].step_count;
            return simulate_path_batch<false>(sequences, states, generator, observe);
        }
    }();
    if (!simulated) return std::unexpected(simulated.error());
    std::array<double, count> payoffs{};
    for (std::size_t i = 0; i < count; ++i) {
        auto& state = states[i];
        const auto& terminal_discount = [&]() -> const MonteCarloDiscount& {
            if constexpr (shared_inputs) return inputs.terminal_discount;
            else return inputs[i].terminal_discount;
        }();
        payoffs[i] = state.payoff ? *state.payoff : settle_autocallable_path(program, terminal_discount, state.spot, state.path);
    }
    return payoffs;
}

template <std::size_t count, bool rates = false, typename Note>
Result<std::array<double, count + (rates ? 2 : 0)>> price_spot_bumps(
    const Note& note, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double h, [[maybe_unused]] CudaWorkspace* workspace = nullptr, double rate_shift = 0.0)
{
    constexpr auto offsets = SpotStencil<count>::offsets;
    const auto program = autocallable_program(note);
    const auto base = autocallable_initial_state(note, context, program);
    constexpr auto outputs = count + (rates ? 2 : 0);
    std::array<PathState, outputs> initial{};
    for (std::size_t i = 0; i < outputs; ++i) {
        const double spot = context.spot_price() + (i < count ? offsets[i] * h : 0.0);
        const double rate = context.model_parameters().risk_free_rate() + (i < count ? 0.0 : i == count ? rate_shift
                                                                                                        : -rate_shift);
        const auto shifted = shifted_context(context, spot, context.model_parameters().volatility(),
                                             rate, context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        const auto state = autocallable_initial_state(note, *shifted, program);
        const auto fixed = state.settlement ? state.settlement : autocallable_fixed_value(note, *shifted, program, state);
        if (fixed) {
            const auto value = checked_price(*fixed);
            if (!value) return std::unexpected(value.error());
        }
        initial[i] = {spot, state.path, fixed};
    }
    std::array<double, outputs> prices{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto inputs = prepare_simulation(note, context, base.next_observation,
                                               program_knock_in_affects_payoff(program) && program.daily_knock_in && !base.path.knocked_in);
        if (!inputs) return std::unexpected(inputs.error());
        if constexpr (rates) {
            std::array<SimulationInputs, 2> rate_inputs;
            std::array<SimulationView, outputs> views{};
            std::fill_n(views.begin(), count, SimulationView{inputs->step_count, inputs->terminal_discount});
            for (std::size_t i = 0; i < rate_inputs.size(); ++i) {
                if (initial[count + i].payoff) continue;
                const auto shifted = shifted_context(context, context.spot_price(), context.model_parameters().volatility(),
                                                     context.model_parameters().risk_free_rate() + (i == 0 ? rate_shift : -rate_shift), context.valuation_time());
                if (!shifted) return std::unexpected(shifted.error());
                auto prepared = prepare_simulation(note, *shifted, base.next_observation,
                                                   program_knock_in_affects_payoff(program) && program.daily_knock_in && !base.path.knocked_in);
                if (!prepared) return std::unexpected(prepared.error());
                rate_inputs[i] = std::move(*prepared);
                views[count + i] = {rate_inputs[i].step_count, rate_inputs[i].terminal_discount};
            }
            const auto values = cpu_path_means<outputs>(settings, [&](auto& generator) { return path_payoffs(program, views, initial, generator); }, inputs->step_count.size());
            if (!values) return std::unexpected(values.error());
            prices = *values;
        } else if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
            const auto values = cuda_active_prices<CudaAutocallableRequest>(initial, [&](const auto& state) { return CudaAutocallableRequest{settings.path_count, *settings.seed, state.spot,
                                                                                                                                             inputs->terminal_discount, program, state.path}; }, [&](auto requests) { return cuda_autocallable_spot_prices(requests, inputs->step_count, workspace); });
            if (!values) return std::unexpected(values.error());
            prices = *values;
#else
            return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
        } else {
            const auto values = cpu_path_means<count>(settings, [&](auto& generator) { return path_payoffs(program, *inputs, initial, generator); }, inputs->step_count.size());
            if (!values) return std::unexpected(values.error());
            prices = *values;
        }
    }
    return checked_path_prices(initial, prices);
}

template <AutocallableProduct Note>
Result<std::array<double, 3>> price_rate_bumps(
    const Note& note, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double h, std::optional<double> base_price = std::nullopt)
{
    const auto program = autocallable_program(note);
    return cpu_rate_prices<SimulationInputs, PathState>(context, settings, h, base_price, [&](const PricingContext& shifted) -> Result<std::pair<SimulationInputs, PathState>> {
            const auto state = autocallable_initial_state(note, shifted, program);
            const auto fixed = state.settlement ? state.settlement : autocallable_fixed_value(note, shifted, program, state);
            SimulationInputs inputs{};
            if (fixed) {
                const auto value = checked_price(*fixed);
                if (!value) return std::unexpected(value.error());
            } else {
                auto prepared = prepare_simulation(note, shifted, state.next_observation,
                    program_knock_in_affects_payoff(program) && program.daily_knock_in && !state.path.knocked_in);
                if (!prepared) return std::unexpected(prepared.error());
                inputs = std::move(*prepared);
            }
            return std::pair{std::move(inputs), PathState{shifted.spot_price(), state.path, fixed}}; }, [&](const auto& inputs, const auto& initial, auto& generator) { return path_payoffs(program, inputs, initial, generator); });
}

template <AutocallableProduct Note>
struct AutocallableMonteCarloRequest {
    AutocallableContract<Note> contract;
#if KIYOSI_HAS_CUDA
    CudaWorkspace storage{};
#endif

    CudaWorkspace* workspace()
    {
#if KIYOSI_HAS_CUDA
        return &storage;
#else
        return nullptr;
#endif
    }
};

template <AutocallableProduct Note>
Result<double> price_autocallable(
    const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    AutocallableMonteCarloRequest<Note>& request)
{
    const auto& note = request.contract.note();
    const auto valid = request.contract.validate(context);
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
            inputs->step_count, request.workspace()));
        if (!mean) return std::unexpected(mean.error());
        return checked_price(*mean);
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }

    const auto prices = cpu_path_means<1>(settings, [&](auto& generator) { return path_payoffs(program, *inputs, std::array{PathState{context.spot_price(), initial.path, {}}}, generator); }, inputs->step_count.size());
    if (!prices) return std::unexpected(prices.error());
    return checked_price((*prices)[0]);
}

template <AutocallableProduct Note>
Result<ImpliedVolatilitySettings> prepare_volatility_request(
    AutocallableMonteCarloRequest<Note>& request, const PricingContext& context,
    ImpliedVolatilitySettings settings)
{
    const auto valid = request.contract.validate(context);
    if (!valid) return std::unexpected(valid.error());
    const auto& note = request.contract.note();
    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);
    if (!initial.settlement && autocallable_fixed_value(note, context, program, initial))
        return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                     "volatility does not affect the remaining cashflows"});
    return settings;
}

} // namespace

template <AutocallableProduct Note>
Result<ImpliedVolatilitySettings> detail::prepare_implied_volatility(
    const MonteCarloAutocallableEngine&, const Note& option, const PricingContext& context,
    ImpliedVolatilitySettings settings)
{
    AutocallableMonteCarloRequest<Note> request{{option, context.calendar()}};
    return prepare_volatility_request(request, context, settings);
}

template <AutocallableProduct Note>
Result<double> MonteCarloAutocallableEngine::price(const Note& note, const PricingContext& context) const
{
    AutocallableMonteCarloRequest<Note> request{{note, context.calendar()}};
    return price_autocallable(context, settings_, request);
}

template <AutocallableProduct Note>
Result<double> implied_volatility(
    const MonteCarloAutocallableEngine& engine, const Note& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    AutocallableMonteCarloRequest<Note> request{{option, context.calendar()}};
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings, [&](const auto& seeded, const Note&, const PricingContext& shifted) { return price_autocallable(shifted, seeded.settings(), request); }, [&](const auto&, ImpliedVolatilitySettings bounds) { return prepare_volatility_request(request, context, bounds); });
}

template <AutocallableProduct Note>
Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const Note& lower, const Note& upper, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings)
{
    for (const auto* note : {&lower, &upper}) {
        AutocallableContract<Note> contract{*note, context.calendar()};
        const auto valid = contract.validate(context);
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
        const auto engine = make_monte_carlo_autocallable_engine(settings);
        if (!engine) return std::unexpected(engine.error());
        const auto first = engine->price(lower, context);
        if (!first) return std::unexpected(first.error());
        const auto second = engine->price(upper, context);
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
    const auto values = cpu_path_means<2>(settings, [&](auto& generator) { return coupon_path_payoffs(context.spot_price(), program, *inputs, initial.path,
                                                                                                      generator, alternate_program, alternate_initial.path); }, inputs->step_count.size());
    if (!values) return std::unexpected(values.error());
    std::array<double, 2> prices{};
    for (std::size_t i = 0; i < prices.size(); ++i) {
        const auto price = checked_price((*values)[i]);
        if (!price) return std::unexpected(price.error());
        prices[i] = *price;
    }
    return prices;
}

template <AutocallableProduct Note>
Result<PricingResult> MonteCarloAutocallableEngine::price_with_greeks(const Note& option, const PricingContext& context,
                                                                      GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(
        *this, option, context, greeks, settings,
        [&](const auto& seeded) -> Result<PricingResult> {
            AutocallableMonteCarloRequest<Note> request{{option, context.calendar()}};
            const auto prices = [&](const PricingContext& shifted) {
                return price_autocallable(shifted, seeded.settings(), request);
            };
            return complete_monte_carlo_greeks(prices, option, context, greeks, settings, seeded.settings().backend, [&](const PricingContext& shifted) { return request.contract.validate(shifted); }, [&]<std::size_t count, bool rates = false>(const PricingContext& shifted, double shift, double rate_shift = 0.0) { return price_spot_bumps<count, rates>(option, shifted, seeded.settings(), shift, request.workspace(), rate_shift); }, [&](double shift, std::optional<double> base_price) { return price_rate_bumps(option, context, seeded.settings(), shift, base_price); });
        });
}

template KIYOSI_EXPORT Result<ImpliedVolatilitySettings> detail::prepare_implied_volatility(
    const MonteCarloAutocallableEngine&, const PhoenixOption&,
    const PricingContext&, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<ImpliedVolatilitySettings> detail::prepare_implied_volatility(
    const MonteCarloAutocallableEngine&, const SnowballOption&,
    const PricingContext&, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<ImpliedVolatilitySettings> detail::prepare_implied_volatility(
    const MonteCarloAutocallableEngine&, const BinarySnowballOption&,
    const PricingContext&, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<ImpliedVolatilitySettings> detail::prepare_implied_volatility(
    const MonteCarloAutocallableEngine&, const TernarySnowballOption&,
    const PricingContext&, ImpliedVolatilitySettings);

template KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine&, const PhoenixOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine&, const SnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine&, const BinarySnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine&, const TernarySnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);

template KIYOSI_EXPORT Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const PhoenixOption&, const PhoenixOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const SnowballOption&, const SnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const BinarySnowballOption&, const BinarySnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);
template KIYOSI_EXPORT Result<std::array<double, 2>> detail::monte_carlo_coupon_prices(
    const TernarySnowballOption&, const TernarySnowballOption&, const PricingContext&, const TradingDayMonteCarloSettings&);

template KIYOSI_EXPORT Result<double> MonteCarloAutocallableEngine::price(const PhoenixOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> MonteCarloAutocallableEngine::price_with_greeks(const PhoenixOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<double> MonteCarloAutocallableEngine::price(const SnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> MonteCarloAutocallableEngine::price_with_greeks(const SnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<double> MonteCarloAutocallableEngine::price(const BinarySnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> MonteCarloAutocallableEngine::price_with_greeks(const BinarySnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<double> MonteCarloAutocallableEngine::price(const TernarySnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> MonteCarloAutocallableEngine::price_with_greeks(const TernarySnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;

} // namespace kiyosi
