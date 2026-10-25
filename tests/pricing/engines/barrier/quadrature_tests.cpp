#include <kiyosi/pricing/engines/barrier/integral.hpp>

#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <cmath>
#include <future>
#include <limits>
#include <numbers>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using namespace kiyosi;
using test::day;

constexpr auto start = day(2025, 1, 1), end = day(2026, 1, 1);

BarrierOption contract(BarrierType kind, OptionType type = OptionType::call, double rebate = 0.0,
                       RebateTiming timing = RebateTiming::at_expiry, std::vector<Date> dates = {},
                       std::optional<BarrierTouchState> history = {})
{
    return make_barrier_option({.option_type = type, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = is_up_barrier(kind) ? 120.0 : 80.0, .barrier_type = kind, .rebate = rebate, .rebate_timing = timing, .observation_mode = dates.empty() ? ObservationMode::continuous : ObservationMode::scheduled, .observation_dates = std::move(dates), .touch_state = history}).value();
}

PricingContext market(double spot = 100.0, double rate = 0.04, Timestamp valuation = start_of_day(start))
{
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar and is returned by value.
    return make_pricing_context(*make_bsm_parameters(rate, 0.01, 0.2), spot, valuation).value();
}

constexpr std::array kinds{BarrierType::up_and_in, BarrierType::up_and_out,
                           BarrierType::down_and_in, BarrierType::down_and_out};
} // namespace

TEST_CASE("Barrier quadrature continuous prices match analytic contracts", "[barrier-quadrature]")
{
    const auto engine = make_quadrature_barrier_engine({1024, 10.0}).value();
    for (const auto kind : kinds)
        for (const auto type : {OptionType::call, OptionType::put})
            for (const double rebate : {0.0, 3.0})
                for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
                    if (is_knock_in_barrier(kind) && timing == RebateTiming::at_hit) continue;
                    const auto option = contract(kind, type, rebate, timing);
                    for (const double rate : {-0.02, 0.04}) {
                        CAPTURE(kind, type, rebate, timing, rate);
                        const auto context = market(100.0, rate);
                        const auto actual = engine.price(option, context);
                        const auto reference = AnalyticBarrierEngine{}.price(option, context);
                        REQUIRE(actual);
                        REQUIRE(reference);
                        CHECK(*actual == Catch::Approx(*reference).margin(3e-8));
                    }
                }
}

TEST_CASE("Barrier quadrature integrates an expiry observation using independent digital moments", "[barrier-quadrature]")
{
    const auto context = market();
    const double discount = std::exp(-0.04);
    const double stock = 100.0 * std::exp(-0.01);
    const auto normal = [](double value) { return 0.5 * std::erfc(-value / std::numbers::sqrt2); };
    const auto above = [&](double strike) {
        const double d2 = (std::log(100.0 / strike) + 0.01) / 0.2;
        return std::array{discount * normal(d2), stock * normal(d2 + 0.2)};
    };
    const auto strike = above(100.0), barrier = above(120.0);
    const double out = strike[1] - barrier[1] - 100.0 * (strike[0] - barrier[0]) + 3.0 * barrier[0];
    const double in = barrier[1] - 100.0 * barrier[0] + 3.0 * (discount - barrier[0]);
    for (const auto settings : {QuadratureSettings{4, 0.1}, QuadratureSettings{512, 8.0}}) {
        const auto engine = make_quadrature_barrier_engine(settings).value();
        CHECK(*engine.price(contract(BarrierType::up_and_out, OptionType::call, 3.0, RebateTiming::at_expiry, {end}), context) == Catch::Approx(out).margin(1e-11));
        CHECK(*engine.price(contract(BarrierType::up_and_in, OptionType::call, 3.0, RebateTiming::at_expiry, {end}), context) == Catch::Approx(in).margin(1e-11));
    }
}

TEST_CASE("Barrier quadrature retains short-maturity and near-barrier values", "[barrier-quadrature]")
{
    const auto engine = make_quadrature_barrier_engine({2048, 10.0}).value();
    for (const auto kind : kinds)
        for (const auto type : {OptionType::call, OptionType::put}) {
            const auto option = contract(kind, type, 1.0, RebateTiming::at_expiry, {}, BarrierTouchState::untouched);
            for (const auto& context : {market(100.0, 0.04, start_of_day(end) - std::chrono::seconds{1}),
                                        market(is_up_barrier(kind) ? 120.0 - 1e-7 : 80.0 + 1e-7,
                                               0.04, start_of_day(end) - std::chrono::days{30})}) {
                CAPTURE(kind, type, context.spot_price());
                const auto actual = engine.price(option, context);
                const auto expected = AnalyticBarrierEngine{}.price(option, context);
                REQUIRE(actual);
                REQUIRE(expected);
                CHECK(*actual == Catch::Approx(*expected).margin(2e-8));
            }
        }
}

TEST_CASE("Barrier quadrature preserves exact dates and early-ending scheduled monitoring", "[barrier-quadrature]")
{
    const auto context = market();
    const auto engine = make_quadrature_barrier_engine({512, 10.0}).value();
    const auto reference = make_finite_difference_barrier_engine({1200, 800}).value();
    for (const auto kind : kinds)
        for (const auto type : {OptionType::call, OptionType::put}) {
            CAPTURE(kind, type);
            const auto option = contract(kind, type, 3.0,
                                         is_knock_in_barrier(kind) ? RebateTiming::at_expiry : RebateTiming::at_hit,
                                         {day(2025, 4, 1), day(2025, 10, 1)});
            const auto actual = engine.price(option, context);
            const auto expected = reference.price(option, context);
            REQUIRE(actual);
            REQUIRE(expected);
            CHECK(*actual == Catch::Approx(*expected).margin(0.025));
        }
    const auto option = contract(BarrierType::up_and_out, OptionType::call, 0.0, RebateTiming::at_expiry,
                                 {day(2025, 4, 1)});
    const auto later = contract(BarrierType::up_and_out, OptionType::call, 0.0, RebateTiming::at_expiry,
                                {day(2025, 10, 1)});
    CHECK(std::abs(*engine.price(option, context) - *engine.price(later, context)) > 0.1);
}

TEST_CASE("Barrier quadrature discounts early observation rebates at their contractual payment time", "[barrier-quadrature]")
{
    const auto observation = day(2025, 4, 1);
    const double time = *year_fraction(start, observation);
    const auto engine = make_quadrature_barrier_engine({512, 10.0}).value();
    for (const double rate : {-0.02, 0.04}) {
        const auto context = market(100.0, rate);
        const double standardized = (std::log(1.2) - (rate - 0.01 - 0.02) * time) / (0.2 * std::sqrt(time));
        const double probability = 0.5 * std::erfc(standardized / std::numbers::sqrt2);
        for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
            CAPTURE(rate, timing);
            const auto option = make_barrier_option({.option_type = OptionType::call, .strike = 1e9, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .rebate = 7.0, .rebate_timing = timing, .observation_mode = ObservationMode::scheduled, .observation_dates = {observation}}).value();
            const auto actual = engine.price(option, context);
            REQUIRE(actual);
            // The vanilla payoff's lognormal tail is below representable probability at this strike.
            const double expected = 7.0 * std::exp(-rate * (timing == RebateTiming::at_hit ? time : 1.0)) * probability;
            CHECK(*actual == Catch::Approx(expected).margin(2e-11));
        }
    }
}

TEST_CASE("Barrier quadrature refines space and integration extent independently", "[barrier-quadrature]")
{
    const auto context = market();
    const auto option = contract(BarrierType::down_and_out, OptionType::put, 1.0, RebateTiming::at_expiry,
                                 {day(2025, 4, 1), day(2025, 7, 1), day(2025, 10, 1)});
    const double reference = *make_quadrature_barrier_engine({1024, 12.0})->price(option, context);
    const double medium = *make_quadrature_barrier_engine({512, 12.0})->price(option, context);
    const double coarse = *make_quadrature_barrier_engine({128, 12.0})->price(option, context);
    CHECK(std::abs(medium - reference) < 2e-5);
    CHECK(std::abs(medium - reference) < std::abs(coarse - reference));
    CHECK(*make_quadrature_barrier_engine({1024, 8.0})->price(option, context) == Catch::Approx(reference).margin(2e-5));

    const auto continuous = contract(BarrierType::down_and_in);
    const double expected = *AnalyticBarrierEngine{}.price(continuous, context);
    const double refined = *make_quadrature_barrier_engine({1024, 10.0})->price(continuous, context);
    CHECK(refined == Catch::Approx(expected).margin(3e-8));
    CHECK(std::abs(refined - expected) < std::abs(*make_quadrature_barrier_engine({8, 10.0})->price(continuous, context) - expected));
    CHECK(std::abs(refined - expected) < std::abs(*make_quadrature_barrier_engine({1024, 1.0})->price(continuous, context) - expected));
}

TEST_CASE("Barrier quadrature resolves touch history current observations and expiry", "[barrier-quadrature]")
{
    const QuadratureBarrierEngine engine;
    const auto observed = day(2025, 4, 1);
    const auto option = contract(BarrierType::up_and_out, OptionType::call, 3.0, RebateTiming::at_hit,
                                 {observed, end});
    const auto boundary = market(120.0, 0.04, start_of_day(observed));
    CHECK(*engine.price(option, boundary) == 3.0);
    const auto greeks = engine.price_with_greeks(option, boundary, true);
    REQUIRE(greeks);
    for (std::size_t index = 0; index < greek_count; ++index)
        CHECK_FALSE(greeks->has(static_cast<Greek>(index)));
    const auto past = market(110.0, 0.04, start_of_day(observed) + std::chrono::hours{1});
    const auto missing = engine.price(option, past);
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == ErrorCategory::invalid_parameter);
    CHECK(*engine.price(*option.replace(BarrierTouchState::touched), past) == 0.0);
    REQUIRE(engine.price(*option.replace(BarrierTouchState::untouched), past));

    const auto stopped = contract(BarrierType::up_and_out, OptionType::call, 0.0, RebateTiming::at_expiry,
                                  {observed}, BarrierTouchState::untouched);
    const auto activated = contract(BarrierType::up_and_in, OptionType::call, 3.0, RebateTiming::at_expiry,
                                    {observed}, BarrierTouchState::untouched);
    const auto european = make_european_option(OptionType::call, 100.0, start, end).value();
    CHECK(*engine.price(stopped, past) == *AnalyticVanillaEngine{}.price(european, past));
    CHECK(*engine.price(activated, past) == Catch::Approx(3.0 * std::exp(-0.04 * *year_fraction(past.valuation_time(), end))).margin(1e-12));
    const auto expiry = market(110.0, 0.04, start_of_day(end));
    CHECK(*engine.price(stopped, expiry) == 10.0);
    CHECK(*engine.price(activated, expiry) == 3.0);
}

TEST_CASE("Barrier quadrature validates schedules before settled pricing", "[barrier-quadrature]")
{
    const auto option = contract(BarrierType::up_and_out, OptionType::call, 3.0, RebateTiming::at_hit,
                                 {day(2025, 1, 4), end}, BarrierTouchState::touched);
    const auto context = market(130.0, 0.04, start_of_day(day(2025, 2, 3)));
    const auto result = QuadratureBarrierEngine{}.price(option, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == ErrorCategory::invalid_date);
    for (const auto settings : {QuadratureSettings{3, 8.0}, QuadratureSettings{512, 0.0},
                                QuadratureSettings{512, std::numeric_limits<double>::infinity()}}) {
        const auto invalid = make_quadrature_barrier_engine(settings);
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
}

TEST_CASE("Barrier quadrature Greeks and implied volatility use the shared analytics contract", "[barrier-quadrature]")
{
    const auto engine = make_quadrature_barrier_engine({1024, 10.0}).value();
    const auto option = contract(BarrierType::down_and_out);
    const auto context = market();
    const GreeksRequest request{Greek::delta, Greek::gamma, Greek::vega, Greek::rho};
    const auto actual = engine.price_with_greeks(option, context, request);
    const auto expected = AnalyticBarrierEngine{}.price_with_greeks(option, context, request);
    REQUIRE(actual);
    REQUIRE(expected);
    CHECK(actual->price() == *engine.price(option, context));
    for (const auto greek : {Greek::delta, Greek::gamma, Greek::vega, Greek::rho}) {
        CAPTURE(greek);
        REQUIRE(actual->has(greek));
        CHECK(test::greek_value(*actual, greek) == Catch::Approx(test::greek_value(*expected, greek)).margin(3e-6));
    }
    CHECK_FALSE(actual->has(Greek::speed));
    const auto implied = implied_volatility(engine, option, context, actual->price(),
                                            {.lower_bound = 0.15, .upper_bound = 0.25, .price_tolerance = 1e-9, .parameter_tolerance = 1e-9});
    REQUIRE(implied);
    CHECK(*implied == Catch::Approx(0.2).margin(1e-8));
    const auto fixed = *contract(BarrierType::up_and_out, OptionType::call, 3.0).replace(BarrierTouchState::touched);
    const auto later = market(100.0, 0.04, start_of_day(day(2025, 2, 3)));
    const auto unidentifiable = implied_volatility(engine, fixed, later, *engine.price(fixed, later));
    REQUIRE_FALSE(unidentifiable);
    CHECK(unidentifiable.error().category == ErrorCategory::unsupported_operation);

    const auto scheduled = contract(BarrierType::up_and_out, OptionType::call, 2.0, RebateTiming::at_expiry,
                                    {day(2025, 4, 1), day(2025, 10, 1)});
    auto concurrent = std::async(std::launch::async, [&] { return engine.price(scheduled, context); });
    const auto price = engine.price(scheduled, context);
    CHECK(*concurrent.get() == *price);
}
