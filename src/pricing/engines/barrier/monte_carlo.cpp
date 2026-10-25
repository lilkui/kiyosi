#include <kiyosi/pricing/engines/barrier/monte_carlo.hpp>
#include <kiyosi/pricing/engines/barrier/double_monte_carlo.hpp>

#include <array>
#include <cmath>
#include <vector>

#include <kiyosi/pricing/engines/barrier/analytic.hpp>
#include <kiyosi/pricing/engines/barrier/double_analytic.hpp>

#include "../../detail/barrier_settlement.hpp"
#include "../../detail/double_barrier.hpp"
#include "../../detail/numerical_greeks.hpp"
#include "../monte_carlo_barrier.hpp"
#include "../monte_carlo_host.hpp"

namespace kiyosi {
namespace {
using namespace detail;

GreeksRequest supported_greeks(GreeksRequest request)
{
    std::array<Greek, 2> values{};
    std::size_t count = 0;
    for (const auto greek : {Greek::delta, Greek::gamma})
        if (request.has(greek)) values[count++] = greek;
    return GreeksRequest{std::span{values}.first(count)};
}

template <typename Option>
Result<PricingResult> price_barrier(const Option& option, const PricingContext& context,
                                    MonteCarloSettings settings, GreeksRequest requested, NumericalShiftSettings shifts)
{
    constexpr bool double_barrier = std::same_as<Option, DoubleBarrierOption>;
    using AnalyticEngine = std::conditional_t<double_barrier, AnalyticDoubleBarrierEngine, AnalyticBarrierEngine>;
    const auto life = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!life) return std::unexpected(life.error());
    const auto& terms = option.barrier_terms();
    if (!terms.is_continuous()) {
        const auto schedule = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const auto greeks = supported_greeks(native_greeks_request(option, context, requested,
                                                               symmetric_shift(context.spot_price(), shifts.spot_shift)));
    if (const auto settled = resolved_barrier_price(option, context)) {
        if (!*settled) return std::unexpected(settled->error());
        if (greeks.empty()) return make_pricing_result(**settled);
        return AnalyticEngine{}.price_with_greeks(option, context, greeks, shifts);
    }
    const auto& model = context.model_parameters();
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    if (time == 0.0) {
        const auto value = AnalyticEngine{}.price(option, context);
        if (!value) return std::unexpected(value.error());
        return make_pricing_result(*value);
    }
    const double exponent = -model.risk_free_rate() * time;
    const double barrier = [&] {
        if constexpr (double_barrier) return option.lower_barrier();
        else return option.barrier_level();
    }();
    const bool up = [&] {
        if constexpr (double_barrier) return false;
        else return terms.is_up();
    }();
    const BarrierMonteCarloParameters parameters{
        context.spot_price(), option.strike(), barrier, std::log(context.spot_price()), std::abs(log_price_ratio(context.spot_price(), barrier)), option.rebate(), {std::exp(exponent), exponent}, option.option_type() == OptionType::call, up, terms.is_knock_in(), option.rebate_timing() == RebateTiming::at_hit, terms.is_continuous(), [&] { if constexpr (double_barrier) return option.upper_barrier(); else return 0.0; }(), [&] { if constexpr (double_barrier) return log_price_ratio(option.upper_barrier(), barrier); else return 0.0; }()};
    std::vector<BarrierMonteCarloStep> steps;
    Timestamp previous = context.valuation_time();
    const auto append = [&](Date date, bool observe) -> Result<void> {
        const auto step = simulation_step(context, previous, date);
        if (!step) return std::unexpected(step.error());
        steps.push_back({*step, observe});
        previous = start_of_day(date);
        return {};
    };
    if (!terms.is_continuous()) {
        for (const auto date : option.observation_dates()) {
            if (start_of_day(date) <= previous) continue;
            const auto added = append(date, true);
            if (!added) return std::unexpected(added.error());
        }
    }
    if (previous < start_of_day(option.expiry_date())) {
        const auto added = append(option.expiry_date(), false);
        if (!added) return std::unexpected(added.error());
    }
    const bool delta = greeks.has(Greek::delta), gamma = greeks.has(Greek::gamma);
    std::array<double, 3> means{};
    if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto values = cuda_barrier_prices({settings.path_count, settings.seed ? *settings.seed : random_seed(),
                                                 parameters, settings.antithetic, delta, gamma},
                                                steps);
        const auto valid = cuda_mean(values.result);
        if (!valid) return std::unexpected(valid.error());
        means = {values.payoff_means[0], values.payoff_means[1], values.payoff_means[2]};
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
    } else {
        const auto values = cpu_path_means<3>(settings, [&](auto& generator) -> Result<std::array<double, 3>> {
            std::normal_distribution<double> normal;
            double log_asset = parameters.log_spot;
            double first_normal = 0.0;
            bool first = true, touched = false;
            double knocked_out = 0.0;
            for (const auto& step : steps) {
                const double z = draw_monte_carlo_normal(generator, normal);
                if (first) first_normal = z;
                first = false;
                log_asset += step.simulation.drift + step.simulation.diffusion * z;
                const double asset = std::exp(log_asset);
                if (!std::isfinite(asset) || asset <= 0.0)
                    return std::unexpected(Error{ErrorCategory::invalid_result, "barrier Monte Carlo path is not representable"});
                if (step.observe && parameters.breached(asset)) touched = true;
                if (touched && !parameters.knock_in) {
                    knocked_out = (parameters.at_hit ? step.simulation.discount : parameters.terminal_discount).apply(parameters.rebate);
                    break;
                }
            }
            const auto sample = parameters.sample(log_asset, first_normal, steps.front().simulation.diffusion,
                                                   touched, knocked_out, delta, gamma);
            const std::array values{sample.price, sample.delta, sample.gamma};
            for (const double value : values)
                if (!std::isfinite(value))
                    return std::unexpected(Error{ErrorCategory::invalid_result, "barrier Monte Carlo produced a non-finite sample"});
            return values; }, steps.size());
        if (!values) return std::unexpected(values.error());
        means = *values;
    }
    if (parameters.continuous && !parameters.knock_in && parameters.at_hit && parameters.rebate != 0.0) {
        const auto hit_value = [&](const PricingContext& shifted) {
            if constexpr (double_barrier) {
                return double_barrier_hit_value(option, shifted);
            } else {
                const auto& market = shifted.model_parameters();
                return barrier_hit_value(option.rebate(), std::abs(log_price_ratio(option.barrier_level(), shifted.spot_price())),
                                         terms.is_up(), market.risk_free_rate() - market.dividend_yield() - 0.5 * market.volatility() * market.volatility(),
                                         market.volatility(), time, market.risk_free_rate());
            }
        };
        const auto rebate = hit_value(context);
        if (!rebate) return std::unexpected(rebate.error());
        means[0] += *rebate;
        if (!greeks.empty()) {
            const auto derivatives = complete_greeks(hit_value, option, context, greeks, shifts, *make_pricing_result(*rebate));
            if (!derivatives) return std::unexpected(derivatives.error());
            if (delta && derivatives->has(Greek::delta)) means[1] += *derivatives->require(Greek::delta);
            if (gamma && derivatives->has(Greek::gamma)) means[2] += *derivatives->require(Greek::gamma);
        }
    }
    return make_pricing_result(means[0], {{Greek::delta, delta ? std::optional{means[1]} : std::nullopt},
                                          {Greek::gamma, gamma ? std::optional{means[2]} : std::nullopt}});
}
} // namespace

Result<double> MonteCarloBarrierEngine::price(const BarrierOption& option, const PricingContext& context) const
{
    return detail::price_value(price_barrier(option, context, settings_, {}, {}));
}

Result<PricingResult> MonteCarloBarrierEngine::price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                                 GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& engine) {
        return price_barrier(option, context, engine.settings(), greeks, settings);
    });
}

Result<double> MonteCarloDoubleBarrierEngine::price(const DoubleBarrierOption& option, const PricingContext& context) const
{
    return detail::price_value(price_barrier(option, context, settings_, {}, {}));
}

Result<PricingResult> MonteCarloDoubleBarrierEngine::price_with_greeks(const DoubleBarrierOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& engine) {
        return price_barrier(option, context, engine.settings(), greeks, settings);
    });
}
} // namespace kiyosi
