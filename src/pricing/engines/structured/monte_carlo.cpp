#include <kiyosi/pricing/engines/structured/monte_carlo.hpp>

#include <cmath>
#include <random>

#include <kiyosi/market/schedule.hpp>

#include "../../detail/autocallable_traits.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

template <typename Note>
double path_payoff(const Note& note, const PricingContext& context, std::mt19937_64& generator)
{
    if (note.touch_status() == barrier_touch_status::up) return 0.0;

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const timestamp valuation = context.valuation_time();
    const auto& dates = note.observation_dates();
    const auto schedule = observation_schedule(note, valuation);

    double value = context.asset_price();
    double coupons = 0.0;
    bool knocked_in = note.touch_status() == barrier_touch_status::down;
    if (valuation == start_of_day(date_of(valuation)))
        knocked_in = is_knocked_in(note, value, knocked_in, valuation == note.expiry());

    std::size_t index = 0;
    if (!schedule.empty() && dates[schedule.front()] == valuation) {
        const auto event = schedule.front();
        const double coupon = observation_coupon(note, event, value);
        if (value >= note.knock_out_prices()[event]) return note.principal_ratio() + coupon;
        if constexpr (carries_observation_coupon<Note>) coupons = coupon;
        index = 1;
    }
    if (valuation == note.expiry()) {
        knocked_in = is_knocked_in(note, value, knocked_in, true);
        return coupons + terminal_settlement(note, value, knocked_in);
    }

    std::normal_distribution<double> normal;
    auto previous = valuation;
    for (const auto current : trading_dates(context.calendar(), valuation, note.expiry())) {
        const double dt = actual_365(previous, current);
        value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt +
                          sigma * std::sqrt(dt) * normal(generator));
        previous = current;
        knocked_in = is_knocked_in(note, value, knocked_in, false);
        if (index >= schedule.size() || dates[schedule[index]] != current) continue;
        const auto event = schedule[index];
        const double time = actual_365(valuation, current);
        const double coupon = observation_coupon(note, event, value);
        if (value >= note.knock_out_prices()[event])
            return (note.principal_ratio() + coupon) * std::exp(-rate * time) + coupons;
        if constexpr (carries_observation_coupon<Note>) coupons += coupon * std::exp(-rate * time);
        ++index;
    }
    knocked_in = is_knocked_in(note, value, knocked_in, true);
    return coupons + std::exp(-rate * actual_365(valuation, note.expiry())) *
                         terminal_settlement(note, value, knocked_in);
}

} // namespace

template <typename Note>
result<PricingResult> MonteCarloStructuredEngine<Note>::price(
    const Note& note, const PricingContext& context) const
{
    auto contract = validate_note(note);
    if (!contract) return std::unexpected(contract.error());
    auto valid = validate_life(context.valuation_time(), note.effective(), note.expiry());
    if (!valid) return std::unexpected(valid.error());
    auto schedule = validate_observation_dates(note.observation_dates(), note.effective(),
                                               note.expiry(), context.calendar());
    if (!schedule) return std::unexpected(schedule.error());
    if (settings_.path_count <= 0 || settings_.path_count > 10'000'000)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "structured Monte Carlo path count is out of range"});

    std::mt19937_64 generator(settings_.seed.value_or(std::random_device{}()));
    double sum = 0.0;
    for (int path = 0; path < settings_.path_count; ++path)
        sum += path_payoff(note, context, generator);
    const double value = sum / static_cast<double>(settings_.path_count);
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result,
                                     "structured pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, value}};
}

template class MonteCarloStructuredEngine<PhoenixOption>;
template class MonteCarloStructuredEngine<SnowballOption>;
template class MonteCarloStructuredEngine<BinarySnowballOption>;
template class MonteCarloStructuredEngine<TernarySnowballOption>;

} // namespace kiyosi
