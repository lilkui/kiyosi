#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <cmath>
#include <vector>

#include <kiyosi/kiyosi.hpp>
#include <kiyosi/pricing/engines/barrier/binomial.hpp>
#include "support/common.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;

double enumerated_barrier(const BarrierOption& option, double spot, double rate, double dividend, double volatility,
                          double time, int steps, const std::vector<bool>& observed, int initial_level = 0, int initial_node = 0)
{
    const double dt = time / steps, jump = volatility * std::sqrt(dt);
    const double probability = (std::exp((rate - dividend) * dt) - std::exp(-jump)) / (std::exp(jump) - std::exp(-jump));
    const int remaining = steps - initial_level;
    double result = 0.0;
    for (int path = 0; path < (1 << remaining); ++path) {
        int node = initial_node, first_hit = -1;
        double weight = 1.0;
        for (int level = initial_level; level <= steps; ++level) {
            const double asset = spot * std::exp((2.0 * node - level) * jump);
            if (first_hit < 0 && observed[level] && option.barrier_terms().is_breached_by(asset)) first_hit = level;
            if (level != steps) {
                const bool up = (path & (1 << (level - initial_level))) != 0;
                node += up ? 1 : 0;
                weight *= up ? probability : 1.0 - probability;
            }
        }
        const double terminal = spot * std::exp((2.0 * node - steps) * jump);
        const double payoff = std::max((option.option_type() == OptionType::call ? 1.0 : -1.0) * (terminal - option.strike()), 0.0);
        double payment = payoff, payment_time = remaining * dt;
        if (option.barrier_terms().is_knock_in() ? first_hit < 0 : first_hit >= 0) {
            payment = option.rebate();
            if (option.rebate_timing() == RebateTiming::at_hit) payment_time = (first_hit - initial_level) * dt;
        }
        result += weight * payment * std::exp(-rate * payment_time);
    }
    return result;
}

TEST_CASE("Barrier CRR prices and node Greeks match enumerated paths", "[barrier-crr]")
{
    const auto start = day(2025, 1, 1), expiry = start + std::chrono::days{10};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.25), 100.0, start, all_days_calendar());
    const double time = 10.0 / 365.0;
    for (const int steps : {1, 2, 5})
        for (const auto kind : {BarrierType::up_and_in, BarrierType::up_and_out, BarrierType::down_and_in, BarrierType::down_and_out})
            for (const auto direction : {OptionType::call, OptionType::put})
                for (const bool scheduled : {false, true})
                    for (const double rebate : {0.0, 2.0})
                        for (const auto timing : {RebateTiming::at_expiry, RebateTiming::at_hit}) {
                            if (is_knock_in_barrier(kind) && timing == RebateTiming::at_hit) continue;
                            CAPTURE(steps, kind, direction, scheduled, rebate, timing);
                            const double jump = 0.25 * std::sqrt(time / steps);
                            const double barrier = 100.0 * std::exp((is_up_barrier(kind) ? 1.5 : -1.5) * jump);
                            const auto option = *make_barrier_option({.option_type = direction, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .barrier_level = barrier, .barrier_type = kind, .rebate = rebate, .rebate_timing = timing, .observation_mode = scheduled ? ObservationMode::scheduled : ObservationMode::continuous, .observation_dates = scheduled ? std::vector{expiry} : std::vector<Date>{}});
                            std::vector<bool> observed(static_cast<std::size_t>(steps) + 1, !scheduled);
                            observed.back() = true;
                            const auto engine = *make_cox_ross_rubinstein_barrier_engine({steps});
                            const auto actual = engine.price_with_greeks(option, context, true);
                            REQUIRE(actual);
                            CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(enumerated_barrier(option, 100.0, 0.04, 0.01, 0.25, time, steps, observed), 2e-12));
                            CHECK(actual->price() == *engine.price(option, context));
                            const double down_value = enumerated_barrier(option, 100.0, 0.04, 0.01, 0.25, time, steps, observed, 1, 0);
                            const double up_value = enumerated_barrier(option, 100.0, 0.04, 0.01, 0.25, time, steps, observed, 1, 1);
                            CHECK_THAT(*actual->require(Greek::delta), Catch::Matchers::WithinAbs((up_value - down_value) / (100.0 * (std::exp(jump) - std::exp(-jump))), 1e-11));
                            CHECK(actual->has(Greek::gamma) == (steps >= 2));
                            if (steps >= 2) {
                                std::array<double, 3> second{};
                                for (int node = 0; node < 3; ++node)
                                    second[node] = enumerated_barrier(option, 100.0, 0.04, 0.01, 0.25, time, steps, observed, 2, node);
                                const double expected = ((second[2] - second[1]) / (100.0 * std::expm1(2.0 * jump)) -
                                                         (second[1] - second[0]) / (-100.0 * std::expm1(-2.0 * jump))) /
                                                        (50.0 * (std::exp(2.0 * jump) - std::exp(-2.0 * jump)));
                                CHECK_THAT(*actual->require(Greek::gamma), Catch::Matchers::WithinAbs(expected, 1e-10));
                            }
                            for (const auto greek : {Greek::vega, Greek::rho, Greek::speed, Greek::theta, Greek::charm, Greek::color, Greek::vanna, Greek::zomma})
                                CHECK_FALSE(actual->has(greek));
                        }
}

TEST_CASE("Barrier CRR scheduled monitoring preserves equality and early ending windows", "[barrier-crr]")
{
    const auto start = day(2025, 1, 1), end = start + std::chrono::days{4};
    const auto context = *make_pricing_context(*make_bsm_parameters(-0.02, 0.01, 0.3), 100.0, start, all_days_calendar());
    for (const auto kind : {BarrierType::up_and_in, BarrierType::up_and_out, BarrierType::down_and_in, BarrierType::down_and_out}) {
        const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 100.0, .barrier_type = kind, .rebate = 2.0, .observation_mode = ObservationMode::scheduled, .observation_dates = {start + std::chrono::days{2}}});
        const auto engine = *make_cox_ross_rubinstein_barrier_engine({4});
        const auto price = engine.price(option, context);
        REQUIRE(price);
        CHECK_THAT(*price, Catch::Matchers::WithinAbs(enumerated_barrier(option, 100.0, -0.02, 0.01, 0.3, 4.0 / 365.0, 4, {false, false, true, false, false}), 1e-11));
    }
}

TEST_CASE("Barrier CRR validates history and resolves settlements before off-grid errors", "[barrier-crr]")
{
    const auto start = day(2025, 1, 1), end = start + std::chrono::days{10};
    auto terms = BarrierOptionTerms{.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .rebate = 3.0, .rebate_timing = RebateTiming::at_hit, .observation_mode = ObservationMode::scheduled, .observation_dates = {start, start + std::chrono::days{1}, end}};
    const auto engine = *make_cox_ross_rubinstein_barrier_engine({3});
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    auto actual = engine.price(*make_barrier_option(terms), context);
    REQUIRE_FALSE(actual);
    CHECK(actual.error().category == ErrorCategory::invalid_parameter);
    terms.touch_state = BarrierTouchState::untouched;
    actual = engine.price(*make_barrier_option(terms), context);
    REQUIRE_FALSE(actual);
    CHECK(actual.error().category == ErrorCategory::unsupported_operation);
    terms.touch_state = BarrierTouchState::touched;
    CHECK(*engine.price(*make_barrier_option(terms), context) == 0.0);
    const auto current = *make_pricing_context(context.model_parameters(), 120.0, start, all_days_calendar());
    terms.touch_state.reset();
    CHECK(*engine.price(*make_barrier_option(terms), current) == 3.0);
    const auto greeks = engine.price_with_greeks(*make_barrier_option(terms), current, true);
    REQUIRE(greeks);
    CHECK_FALSE(greeks->has(Greek::delta));
    CHECK_FALSE(greeks->has(Greek::gamma));
    CHECK_FALSE(make_cox_ross_rubinstein_barrier_engine({0}));
    CHECK_FALSE(engine.price_with_greeks(*make_barrier_option(terms), current, {}, {.spot_shift = 0.0}));
}

TEST_CASE("Barrier CRR continuous refinement approaches analytic prices", "[barrier-crr]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    for (const auto kind : {BarrierType::up_and_in, BarrierType::up_and_out, BarrierType::down_and_in, BarrierType::down_and_out}) {
        const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = is_up_barrier(kind) ? 120.0 : 80.0, .barrier_type = kind, .rebate = 2.0});
        const double expected = *AnalyticBarrierEngine{}.price(option, context);
        double largest_fine_error = 0.0;
        for (const int steps : {1023, 1024, 2047, 2048}) {
            const auto price = make_cox_ross_rubinstein_barrier_engine({steps})->price(option, context);
            REQUIRE(price);
            largest_fine_error = std::max(largest_fine_error, std::abs(*price - expected));
        }
        CHECK(largest_fine_error < 0.25);
    }
}

TEST_CASE("Barrier CRR aligns intraday valuation timestamps exactly", "[barrier-crr]")
{
    const auto start = day(2025, 1, 1), end = start + std::chrono::days{4};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0,
                                               start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {start + std::chrono::days{2}}});
    const auto engine = *make_cox_ross_rubinstein_barrier_engine({7});
    const auto price = engine.price(option, context);
    REQUIRE(price);
    CHECK_THAT(*price, Catch::Matchers::WithinAbs(enumerated_barrier(option, 100.0, 0.04, 0.01, 0.2, 3.5 / 365.0, 7, {false, false, false, true, false, false, false, false}), 1e-11));
    const auto shifted = *make_pricing_context(context.model_parameters(), 100.0, context.valuation_time() + std::chrono::microseconds{1}, all_days_calendar());
    const auto rejected = engine.price(option, shifted);
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::unsupported_operation);
}

TEST_CASE("Barrier CRR includes noncentral node equality in scheduled touches", "[barrier-crr]")
{
    const auto start = day(2025, 1, 1), end = start + std::chrono::days{2};
    for (const double volatility : {0.1, 0.2, 0.3, 0.4})
        for (const bool up : {false, true}) {
            CAPTURE(volatility, up);
            const double jump = volatility * std::sqrt(1.0 / 365.0);
            const double barrier = 100.0 * std::exp(up ? jump : -jump);
            const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, volatility), 100.0, start, all_days_calendar());
            const auto option = *make_barrier_option({.option_type = up ? OptionType::call : OptionType::put, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = barrier, .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {start + std::chrono::days{1}}});
            const auto actual = make_cox_ross_rubinstein_barrier_engine({2})->price(option, context);
            REQUIRE(actual);
            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(enumerated_barrier(option, 100.0, 0.04, 0.01, volatility, 2.0 / 365.0, 2, {false, true, false}), 1e-12));
        }
}

TEST_CASE("Barrier CRR implied volatility respects admissible tree bounds", "[barrier-crr]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out});
    const auto engine = *make_cox_ross_rubinstein_barrier_engine({256});
    const auto observed = *engine.price(option, context);
    const auto solved = implied_volatility(engine, option, context, observed, {.lower_bound = 1e-8, .upper_bound = 0.5});
    REQUIRE(solved);
    const auto repriced = engine.price(option, *make_pricing_context(*make_bsm_parameters(0.04, 0.01, *solved), 100.0, start));
    REQUIRE(repriced);
    CHECK_THAT(*repriced, Catch::Matchers::WithinAbs(observed, 2e-7));
    const auto rejected = implied_volatility(engine, option, context, observed, {.lower_bound = 0.0001, .upper_bound = 0.0002});
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::unbracketed_volatility);
}
} // namespace
