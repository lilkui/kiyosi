#include <catch2/catch_test_macros.hpp>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <numeric>
#include <type_traits>
#include <utility>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;

const auto effective_date = day(2025, 1, 6);
const auto expiry_date = effective_date + std::chrono::days{91};

struct Scenario {
    const char* name;
    double spot = 100.0;
    double volatility = 0.25;
    KnockInObservationMode knock_in_observation_mode = KnockInObservationMode::every_trading_day;
    AutocallableBarrierState history = AutocallableBarrierState::none;
    double accumulated_quantity = 3.0;
};

template <typename T>
T checked(Result<T> value)
{
    INFO((value ? "valid result" : value.error().message));
    REQUIRE(value.has_value());
    return *value;
}

double price(Result<double> value)
{
    const double amount = checked(std::move(value));
    REQUIRE(std::isfinite(amount));
    return amount;
}

template <typename Instrument>
double fd_price(const Instrument& instrument, const PricingContext& market,
                FiniteDifferenceSettings settings)
{
    if constexpr (std::is_same_v<Instrument, Accumulator>)
        return price(FiniteDifferenceAccumulatorEngine{settings}.price(instrument, market));
    else
        return price(FiniteDifferenceAutocallableEngine<Instrument>{settings}.price(instrument, market));
}

template <typename Instrument>
double mc_price(const Instrument& instrument, const PricingContext& market,
                TradingDayMonteCarloSettings settings)
{
    if constexpr (std::is_same_v<Instrument, Accumulator>)
        return price(MonteCarloAccumulatorEngine{settings}.price(instrument, market));
    else
        return price(MonteCarloAutocallableEngine<Instrument>{settings}.price(instrument, market));
}

template <typename Instrument>
void compare(const Instrument& instrument, const PricingContext& market,
             double fd_budget, double mc_budget, bool extended)
{
    // Separate fixed budgets prevent noisy MC or unstable FD from excusing disagreement.
    const int fine_steps = extended ? 1600 : 1200;
    const int time_step_count = extended ? 1600 : 960;
    // Daily KO confines accumulator continuation. A denser domain resolves the
    // current-day jump near 110 within the engine's 2000-node limit.
    const double upper = extended && std::is_same_v<Instrument, Accumulator> ? 128.0 : 300.0;
    constexpr auto scheme = FiniteDifferenceScheme::crank_nicolson;
    CAPTURE(fine_steps, time_step_count, upper);
    const double coarse = fd_price(instrument, market, {fine_steps / 4, time_step_count / 4, scheme, upper});
    const double medium = fd_price(instrument, market, {fine_steps / 2, time_step_count / 2, scheme, upper});
    const double fine = fd_price(instrument, market, {fine_steps, time_step_count, scheme, upper});
    // Preserve spatial spacing when widening the domain, isolating truncation error.
    const double wide = fd_price(instrument, market,
                                 {extended ? 2000 : 1800, time_step_count, scheme, upper * (extended ? 1.25 : 1.5)});
    const double grid_change = std::abs(fine - medium);
    const double domain_change = std::abs(wide - fine);
    CAPTURE(coarse, medium, fine, wide, grid_change, domain_change, fd_budget, mc_budget);
    CHECK(grid_change <= fd_budget);
    CHECK(grid_change <= std::max(std::abs(medium - coarse), fd_budget * 0.1));
    CHECK(domain_change <= fd_budget * 0.1);

    constexpr std::array<std::uint64_t, 12> seeds{
        7, 43, 109, 271, 619, 1237, 2539, 5113, 10243, 20507, 40961, 81929};
    const int paths = extended ? (std::is_same_v<Instrument, Accumulator> ? 524288 : 131072) : 32768;
    std::array<double, seeds.size()> batches{};
    for (std::size_t index = 0; index < seeds.size(); ++index) {
        CAPTURE(index, seeds[index], paths);
        batches[index] = mc_price(instrument, market, {paths, seeds[index]});
    }
    const double count = static_cast<double>(batches.size());
    const double mean = std::accumulate(batches.begin(), batches.end(), 0.0) / count;
    double squared_deviations = 0.0;
    for (const double batch : batches)
        squared_deviations += (batch - mean) * (batch - mean);
    const double standard_error = std::sqrt(squared_deviations / (count * (count - 1.0)));
    // Four batch standard errors are a conservative regression envelope, not a proof of accuracy.
    const double uncertainty = 4.0 * standard_error;
    const double discrepancy = std::abs(fine - mean);
    CAPTURE(seeds, paths, batches, mean, standard_error, uncertainty, discrepancy);
    CHECK(uncertainty <= mc_budget);
    CHECK(discrepancy <= fd_budget + uncertainty);
    CHECK(discrepancy <= std::abs(coarse - mean) + uncertainty);
}

template <typename Terms>
Terms note_terms(const Scenario& scenario)
{
    Terms terms;
    terms.initial_spot = 100.0;
    terms.knock_out_levels = {112.0, 108.0, 104.0};
    terms.upper_strike = 100.0;
    terms.lower_strike = 60.0;
    terms.observation_dates = {effective_date + std::chrono::days{30},
                               effective_date + std::chrono::days{60}, expiry_date};
    terms.barrier_state = scenario.history;
    terms.effective_date = effective_date;
    terms.expiry_date = expiry_date;
    if constexpr (requires { terms.knock_in_level; }) {
        terms.knock_in_level = 80.0;
        terms.knock_in_observation_mode = scenario.knock_in_observation_mode;
    }
    if constexpr (requires { terms.knock_out_coupon_rates; }) {
        terms.knock_out_coupon_rates = {0.12, 0.10, 0.08};
        terms.maturity_coupon_rate = 0.06;
    }
    if constexpr (requires { terms.minimum_coupon_rate; }) terms.minimum_coupon_rate = 0.01;
    if constexpr (requires { terms.coupon_rate; }) {
        terms.coupon_rate = 0.0025; // 0.25 native price units per qualifying observation.
        terms.coupon_barrier_levels = {90.0, 90.0, 90.0};
    }
    return terms;
}

void check_scenario(const Scenario& scenario, bool extended)
{
    CAPTURE(scenario.name, scenario.spot, scenario.volatility, scenario.knock_in_observation_mode,
            scenario.history, scenario.accumulated_quantity);
    const auto market = checked(make_pricing_context(
        checked(make_bsm_parameters(0.04, 0.01, scenario.volatility)), scenario.spot, effective_date));
    // Native price units: normalized snowballs, Phoenix cash coupons, accumulator quantity*price.
    DYNAMIC_SECTION(scenario.name << ": snowball")
    {
        compare(checked(make_snowball_option(note_terms<SnowballTerms>(scenario))), market,
                0.003, extended ? 0.002 : 0.004, extended);
    }
    DYNAMIC_SECTION(scenario.name << ": binary snowball")
    {
        compare(checked(make_binary_snowball_option(note_terms<BinarySnowballTerms>(scenario))), market,
                0.0003, extended ? 0.00015 : 0.0003, extended);
    }
    DYNAMIC_SECTION(scenario.name << ": ternary snowball")
    {
        compare(checked(make_ternary_snowball_option(note_terms<TernarySnowballTerms>(scenario))), market,
                0.0005, extended ? 0.0002 : 0.0004, extended);
    }
    DYNAMIC_SECTION(scenario.name << ": phoenix")
    {
        compare(checked(make_phoenix_option(note_terms<PhoenixTerms>(scenario))), market,
                0.008, extended ? 0.004 : 0.008, extended);
    }
    DYNAMIC_SECTION(scenario.name << ": accumulator")
    {
        const auto instrument = checked(make_accumulator({.strike = 100.0,
                                                          .knock_out_level = 110.0,
                                                          .daily_quantity = 1.0,
                                                          .acceleration_factor = 2.0,
                                                          .accumulated_quantity = scenario.accumulated_quantity,
                                                          .effective_date = effective_date,
                                                          .expiry_date = expiry_date}));
        compare(instrument, market, 2.0, extended ? 4.0 : 8.0, extended);
    }
}
} // namespace

TEST_CASE("FD-MC baseline prices agree within bounded numerical error", "[cross-validation]")
{
    check_scenario({"baseline"}, false);
}

TEST_CASE("FD-MC extended prices agree near barriers and with historical state",
          "[cross-validation][.extended]")
{
    for (const auto& scenario : {
             Scenario{"near knock-in", 80.1},
             Scenario{"near coupon barrier", 90.1},
             Scenario{"near knock-out", 109.9},
             Scenario{"expiry_date-only knock-in", 100.0, 0.35, KnockInObservationMode::at_expiry},
             Scenario{"historical knock-in and accumulated quantity", 95.0, 0.25,
                      KnockInObservationMode::every_trading_day, AutocallableBarrierState::knocked_in, 30.0}}) {
        check_scenario(scenario, true);
    }
}
