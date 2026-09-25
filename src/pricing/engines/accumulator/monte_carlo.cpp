#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>

#include <cmath>
#include <new>
#include <optional>
#include <random>
#include <vector>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../monte_carlo_cuda.hpp"

namespace kiyosi {
using namespace detail;

namespace {

using SimulationStep = detail::CudaSimulationStep;

struct InitialState {
    double quantity;
    std::optional<double> settlement;
};

InitialState initial_state(const Accumulator& option, const PricingContext& context)
{
    const Timestamp valuation = context.valuation_time();
    const double value = context.spot_price();
    double quantity = option.accumulated_quantity();
    if (valuation == start_of_day(date_of(valuation)) &&
        context.calendar().is_trading_day(date_of(valuation))) {
        if (value >= option.knock_out_level())
            return {quantity, quantity * (value - option.strike())};
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                            : option.daily_quantity();
    }
    if (valuation == option.expiry_date()) return {quantity, quantity * (value - option.strike())};
    return {quantity, std::nullopt};
}

std::vector<SimulationStep> prepare_simulation(const Accumulator& option,
                                               const PricingContext& context)
{
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    const Timestamp valuation = context.valuation_time();
    const auto dates = trading_dates(context.calendar(), valuation, option.expiry_date());
    std::vector<SimulationStep> steps;
    steps.reserve(dates.size());
    auto previous = valuation;
    for (const Date current : dates) {
        const double dt = actual_365_fixed_year_fraction(previous, current);
        steps.push_back({(rate - dividend - 0.5 * sigma * sigma) * dt,
                         sigma * std::sqrt(dt),
                         std::exp(-rate * actual_365_fixed_year_fraction(valuation, current))});
        previous = current;
    }
    return steps;
}

double path_payoff(const Accumulator& option, const PricingContext& context,
                   const std::vector<SimulationStep>& steps, double quantity,
                   std::mt19937_64& generator)
{
    double value = context.spot_price();
    double terminal = value;

    std::normal_distribution<double> normal;
    double discount = 1.0;
    for (const auto& step : steps) {
        value *= std::exp(step.drift + step.diffusion * normal(generator));
        discount = step.discount;
        if (value >= option.knock_out_level()) {
            terminal = value;
            break;
        }
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                            : option.daily_quantity();
        terminal = value;
    }
    return quantity * (terminal - option.strike()) * discount;
}

#if KIYOSI_HAS_CUDA
std::uint64_t random_seed()
{
    std::random_device source;
    return (static_cast<std::uint64_t>(source()) << 32U) ^
           static_cast<std::uint64_t>(source());
}

Result<double> cuda_sum(detail::CudaPricingResult cuda_result)
{
    switch (cuda_result.status) {
    case detail::CudaPricingStatus::success:
        return cuda_result.payoff_sum;
    case detail::CudaPricingStatus::unavailable:
        return std::unexpected(Error{ErrorCategory::backend_unavailable, cuda_result.message});
    case detail::CudaPricingStatus::failure:
        return std::unexpected(Error{ErrorCategory::backend_failure, cuda_result.message});
    case detail::CudaPricingStatus::out_of_memory:
        throw std::bad_alloc{};
    case detail::CudaPricingStatus::invalid_result:
        return std::unexpected(Error{ErrorCategory::invalid_result, cuda_result.message});
    }
    return std::unexpected(Error{ErrorCategory::backend_failure,
                                 "CUDA Monte Carlo returned an unknown status"});
}
#endif

} // namespace

Result<PricingResult> MonteCarloAccumulatorEngine::price_native(
    const Accumulator& option, const PricingContext& context) const
{
    auto contract = make_accumulator({option.strike(), option.knock_out_level(), option.daily_quantity(),
                                      option.acceleration_factor(), option.accumulated_quantity(),
                                      option.effective_date(), option.expiry_date()});
    if (!contract) return std::unexpected(contract.error());
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (settings_.path_count <= 0 || settings_.path_count > 10'000'000)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "structured Monte Carlo path count is out of range"});
    if (settings_.backend != MonteCarloBackend::cpu &&
        settings_.backend != MonteCarloBackend::cuda)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "Monte Carlo backend is invalid"});

    const auto make_result = [](double value) -> Result<PricingResult> {
        if (!std::isfinite(value))
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "structured pricing produced a non-finite result"});
        return make_pricing_result({{RiskMeasure::price, value}});
    };
    const auto initial = initial_state(option, context);
    if (initial.settlement) return make_result(*initial.settlement);

    const auto steps = prepare_simulation(option, context);
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto sum = cuda_sum(detail::cuda_accumulator_price(
            {settings_.path_count, settings_.seed ? *settings_.seed : random_seed(),
             context.spot_price(), option.strike(), option.knock_out_level(),
             option.daily_quantity(), option.acceleration_factor(), initial.quantity},
            steps));
        if (!sum) return std::unexpected(sum.error());
        return make_result(*sum / static_cast<double>(settings_.path_count));
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }
    std::mt19937_64 generator(settings_.seed ? *settings_.seed : std::random_device{}());
    double sum = 0.0;
    for (int path = 0; path < settings_.path_count; ++path)
        sum += path_payoff(option, context, steps, initial.quantity, generator);
    return make_result(sum / static_cast<double>(settings_.path_count));
}

} // namespace kiyosi
