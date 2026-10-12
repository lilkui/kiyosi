#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <atomic>
#include <cmath>
#include <future>
#include <memory>
#include <map>
#include <string>
#include <tuple>
#include <utility>
#include <vector>

#include "support/reference_harness.hpp"

using kiyosi::test::barrier_kinds;
using kiyosi::test::check_price;
using kiyosi::test::fixture_date;
using kiyosi::test::fixture_number;
using kiyosi::test::measures;

TEST_CASE("Scheduled finite-difference Greeks share calendar validation", "[fd-schedule]")
{
    using namespace kiyosi;
    const auto scheme = GENERATE(FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson);
    const auto kind = GENERATE(BarrierType::down_and_out, BarrierType::down_and_in, BarrierType::up_and_out, BarrierType::up_and_in);
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2035} / 1 / 1};
    std::vector<Date> dates;
    for (auto date = start; date <= end; date += std::chrono::days{1})
        if (weekdays_calendar().is_trading_day(date)) dates.push_back(date);
    const auto queries = std::make_shared<std::atomic<std::size_t>>(0);
    const auto allowed = std::make_shared<std::atomic<bool>>(true);
    const auto calendar = *make_trading_calendar([=](Date date) {
        ++*queries;
        return *allowed && weekdays_calendar().is_trading_day(date);
    }, 252);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(end) - std::chrono::hours{12}, calendar);
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end,
                                              .barrier_level = is_up_barrier(kind) ? 120.0 : 80.0, .barrier_type = kind,
                                              .observation_mode = ObservationMode::scheduled, .observation_dates = dates, .touch_state = BarrierTouchState::untouched});
    const FiniteDifferenceBarrierEngine engine{80, 1, scheme};
    const GreeksRequest request{Greek::vega, Greek::rho, Greek::vanna, Greek::zomma};
    const auto price = engine.price(option, context);
    REQUIRE(price);
    CHECK(queries->exchange(0) == dates.size());
    const auto result = engine.price_with_greeks(option, context, request);
    REQUIRE(result);
    CHECK(result->price() == *price);
    CHECK(queries->exchange(0) == dates.size());
    for (const auto greek : {Greek::vega, Greek::rho, Greek::vanna, Greek::zomma})
        REQUIRE(result->has(greek));

    const NumericalShiftSettings shifts{};
    const double vol_shift = (0.2 + shifts.volatility_shift) - 0.2;
    const double rate_shift = (0.04 + shifts.rate_shift) - 0.04;
    const auto shifted = [&](double volatility, double rate) {
        return *make_pricing_context(*make_bsm_parameters(rate, 0.01, volatility), context.spot_price(), context.valuation_time(), calendar);
    };
    const FiniteDifferenceBarrierEngine fixed{{80, 1, scheme, 4.0 * context.spot_price()}};
    const auto high = fixed.price_with_greeks(option, shifted(0.2 + vol_shift, 0.04), {Greek::delta, Greek::gamma});
    const auto low = fixed.price_with_greeks(option, shifted(0.2 - vol_shift, 0.04), {Greek::delta, Greek::gamma});
    REQUIRE(high);
    REQUIRE(low);
    const auto rate_high = fixed.price(option, shifted(0.2, 0.04 + rate_shift));
    const auto rate_low = fixed.price(option, shifted(0.2, 0.04 - rate_shift));
    REQUIRE(rate_high);
    REQUIRE(rate_low);
    CHECK(*result->require(Greek::vega) == (high->price() - low->price()) / (200.0 * vol_shift));
    CHECK(*result->require(Greek::rho) == (*rate_high - *rate_low) / (200.0 * rate_shift));
    CHECK(*result->require(Greek::vanna) == (*high->require(Greek::delta) - *low->require(Greek::delta)) / (200.0 * vol_shift));
    CHECK(*result->require(Greek::zomma) == (*high->require(Greek::gamma) - *low->require(Greek::gamma)) / (200.0 * vol_shift));
    CHECK_FALSE(result->has(Greek::delta));

    *queries = 0;
    auto task = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, request); });
    const auto concurrent = engine.price_with_greeks(option, context, request);
    const auto first = task.get();
    REQUIRE(concurrent);
    REQUIRE(first);
    CHECK(concurrent->price() == result->price());
    CHECK(first->price() == result->price());
    CHECK(concurrent->values_view() == result->values_view());
    CHECK(first->values_view() == result->values_view());
    CHECK(queries->exchange(0) == 2 * dates.size());

    *allowed = false;
    const auto rejected = engine.price_with_greeks(option, context, request);
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_date);
    *allowed = true;
    *queries = 0;
    const auto recovered = engine.price_with_greeks(option, context, request);
    REQUIRE(recovered);
    CHECK(recovered->price() == result->price());
    CHECK(recovered->values_view() == result->values_view());
    CHECK(queries->exchange(0) == dates.size());
}

TEST_CASE("Scheduled finite-difference Greeks retain shifted validation", "[fd-schedule]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end,
                                              .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out,
                                              .observation_mode = ObservationMode::scheduled, .observation_dates = {start, end}, .touch_state = BarrierTouchState::untouched});
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(end) - std::chrono::hours{12}, all_days_calendar());
    const FiniteDifferenceBarrierEngine engine{80, 1, FiniteDifferenceScheme::explicit_euler};
    const auto price = engine.price(option, context);
    REQUIRE(price);
    const auto rejected = engine.price_with_greeks(option, context, {Greek::vega}, {.volatility_shift = 0.19});
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_parameter);
    const auto recovered = engine.price_with_greeks(option, context, {Greek::vega});
    REQUIRE(recovered);
    CHECK(recovered->price() == *price);
    CHECK(recovered->has(Greek::vega));
}

TEST_CASE("Continuous up barriers preserve put boundaries and rebate timing", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start, all_days_calendar());
    const GreeksRequest request{Greek::delta, Greek::gamma, Greek::speed};
    // References before skipping the upper boundary's discarded vanilla formula.
    for (const auto& [scheme, timing, price, delta, gamma, speed] : {
             std::tuple{FiniteDifferenceScheme::crank_nicolson, RebateTiming::at_hit, 6.001295273660669, -0.46100327414769354, 0.012094195366788239, -0.000720165080800371},
             std::tuple{FiniteDifferenceScheme::crank_nicolson, RebateTiming::at_expiry, 5.965452933440826, -0.46428370491099064, 0.01188517039188602, -0.0007257293326437646},
             std::tuple{FiniteDifferenceScheme::implicit_euler, RebateTiming::at_hit, 6.000291212030822, -0.46094055012576535, 0.012105234989577161, -0.0007214011673570921},
             std::tuple{FiniteDifferenceScheme::implicit_euler, RebateTiming::at_expiry, 5.964442597640509, -0.46422044616223535, 0.011896243851421412, -0.0007269705471180208},
             std::tuple{FiniteDifferenceScheme::explicit_euler, RebateTiming::at_hit, 6.002297864958332, -0.4610659998773655, 0.012083194022974213, -0.0007189321115608181},
             std::tuple{FiniteDifferenceScheme::explicit_euler, RebateTiming::at_expiry, 5.966461797161821, -0.46434696546546267, 0.011874135281227763, -0.0007244912412884592}}) {
        CAPTURE(scheme, timing);
        const auto option = *make_barrier_option({.option_type = OptionType::put, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 110.0, .barrier_type = BarrierType::up_and_out, .rebate = 2.0, .rebate_timing = timing});
        const FiniteDifferenceBarrierEngine engine{{80, 1000, scheme, 400.0}};
        const auto result = engine.price_with_greeks(option, context, request);
        REQUIRE(result);
        for (const auto greek : {Greek::delta, Greek::gamma, Greek::speed})
            REQUIRE(result->has(greek));
        CHECK_THAT(result->price(), Catch::Matchers::WithinAbs(price, 1e-11));
        CHECK_THAT(*result->require(Greek::delta), Catch::Matchers::WithinAbs(delta, 1e-12));
        CHECK_THAT(*result->require(Greek::gamma), Catch::Matchers::WithinAbs(gamma, 1e-12));
        CHECK_THAT(*result->require(Greek::speed), Catch::Matchers::WithinAbs(speed, 1e-11));
        CHECK(*engine.price(option, context) == result->price());
    }
}

TEST_CASE("Continuous barrier fixed edges retain off-node prices and Greeks", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start);
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 110.0, .barrier_type = BarrierType::up_and_out, .rebate = 2.0});
    // Full-grid results before restricting the solve to unconstrained interior rows.
    for (const auto& [scheme, price, delta, gamma, speed] : {
             std::tuple{FiniteDifferenceScheme::crank_nicolson, 1.374681875919217, 0.057342386651520846, 7.95571374527515e-6, -9.302439092745403e-5},
             std::tuple{FiniteDifferenceScheme::implicit_euler, 1.374383050055775, 0.057379104672503844, 6.7888065698046285e-6, -9.315599817910664e-5},
             std::tuple{FiniteDifferenceScheme::explicit_euler, 1.3747121867706569, 0.057338758234461294, 8.050051664994229e-6, -9.301081278631506e-5}}) {
        CAPTURE(scheme);
        const FiniteDifferenceBarrierEngine engine{{200, scheme == FiniteDifferenceScheme::explicit_euler ? 2000 : 200, scheme, 400.0}};
        const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::speed});
        REQUIRE(result);
        for (const auto greek : {Greek::delta, Greek::gamma, Greek::speed})
            REQUIRE(result->has(greek));
        CHECK_THAT(result->price(), Catch::Matchers::WithinAbs(price, 1e-12));
        CHECK_THAT(*result->require(Greek::delta), Catch::Matchers::WithinAbs(delta, 1e-12));
        CHECK_THAT(*result->require(Greek::gamma), Catch::Matchers::WithinAbs(gamma, 1e-12));
        CHECK_THAT(*result->require(Greek::speed), Catch::Matchers::WithinAbs(speed, 1e-11));
        CHECK(*engine.price(option, context) == result->price());
    }
}

TEST_CASE("Scheduled finite-difference knock-in layers preserve rebate parity", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, all_days_calendar());
    for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson}) {
        const FiniteDifferenceSettings settings{80, 400, scheme, 400.0};
        const FiniteDifferenceBarrierEngine engine{settings};
        for (const auto type : {OptionType::call, OptionType::put}) {
            const auto vanilla = FiniteDifferenceVanillaEngine{settings}.price_with_greeks(
                *make_european_option(type, 100.0, start, end), context, {Greek::delta, Greek::gamma});
            REQUIRE(vanilla);
            for (const bool up : {false, true})
                for (const double rebate : {0.0, 2.0})
                    for (const bool observes_expiry : {false, true}) {
                        CAPTURE(scheme, type, up, rebate, observes_expiry);
                        std::vector<Date> dates{start + std::chrono::days{73}, start + std::chrono::days{219}};
                        if (observes_expiry) dates.push_back(end);
                        BarrierOptionTerms terms{.option_type = type, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 120.0 : 80.0, .barrier_type = up ? BarrierType::up_and_in : BarrierType::down_and_in, .rebate = rebate, .observation_mode = ObservationMode::scheduled, .observation_dates = dates};
                        const auto knock_in = *make_barrier_option(terms);
                        terms.barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out;
                        const auto knock_out = *make_barrier_option(terms);
                        const auto in = engine.price_with_greeks(knock_in, context, {Greek::delta, Greek::gamma});
                        const auto out = engine.price_with_greeks(knock_out, context, {Greek::delta, Greek::gamma});
                        REQUIRE(in);
                        REQUIRE(out);
                        CHECK(in->price() == *engine.price(knock_in, context));
                        CHECK_THAT(in->price() + out->price(), Catch::Matchers::WithinAbs(vanilla->price() + rebate * std::exp(-0.04), 1e-10));
                        for (const auto greek : {Greek::delta, Greek::gamma}) {
                            REQUIRE(in->has(greek));
                            REQUIRE(out->has(greek));
                            CHECK_THAT(*in->require(greek) + *out->require(greek), Catch::Matchers::WithinAbs(*vanilla->require(greek), 1e-12));
                        }
                    }
        }
    }
}

TEST_CASE("Finite-difference barrier spot Greeks reuse one solve", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto queries = std::make_shared<int>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, calendar);
    const auto option = *make_barrier_option(
        {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}});
    const FiniteDifferenceBarrierEngine engine{800, 400};
    const auto price = engine.price(option, context);
    REQUIRE(price);
    const int price_queries = std::exchange(*queries, 0);
    REQUIRE(price_queries > 0);
    const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::speed});
    REQUIRE(result);
    CHECK(*queries == price_queries);
    CHECK(result->price() == *price);
    const auto vanilla = *make_european_option(OptionType::call, 100.0, start, end);
    const auto expected = AnalyticVanillaEngine{}.price_with_greeks(vanilla, context, {Greek::delta, Greek::gamma});
    REQUIRE(expected);
    REQUIRE(result->has(Greek::delta));
    REQUIRE(result->has(Greek::gamma));
    REQUIRE(result->has(Greek::speed));
    CHECK_THAT(*result->require(Greek::delta), Catch::Matchers::WithinAbs(*expected->require(Greek::delta), 0.001));
    CHECK_THAT(*result->require(Greek::gamma), Catch::Matchers::WithinAbs(*expected->require(Greek::gamma), 0.0001));
    CHECK_FALSE(result->has(Greek::vega));
}

TEST_CASE("Finite-difference barrier native Greeks respect monitored boundaries", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto parameters = *make_bsm_parameters(0.04, 0.01, 0.2);
    const FiniteDifferenceBarrierEngine engine{800, 400};
    for (const auto kind : {BarrierType::down_and_in, BarrierType::down_and_out, BarrierType::up_and_in, BarrierType::up_and_out}) {
        const bool up = kind == BarrierType::up_and_in || kind == BarrierType::up_and_out;
        const double barrier = up ? 120.0 : 80.0;
        const auto option = *make_barrier_option(
            {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = barrier, .barrier_type = kind});
        const auto context = *make_pricing_context(parameters, 100.0, start);
        const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        const auto expected = AnalyticBarrierEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(actual);
        REQUIRE(expected);
        REQUIRE(actual->has(Greek::delta));
        REQUIRE(actual->has(Greek::gamma));
        CAPTURE(kind);
        CHECK_THAT(*actual->require(Greek::delta), Catch::Matchers::WithinAbs(*expected->require(Greek::delta), 0.003));
        CHECK_THAT(*actual->require(Greek::gamma), Catch::Matchers::WithinAbs(*expected->require(Greek::gamma), 0.0005));
        for (const double spot : {barrier, barrier + (up ? -0.001 : 0.001)}) {
            const auto boundary = *make_pricing_context(parameters, spot, start);
            const auto result = engine.price_with_greeks(option, boundary, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK(result->price() == *engine.price(option, boundary));
            CHECK_FALSE(result->has(Greek::delta));
            CHECK_FALSE(result->has(Greek::gamma));
        }
    }
}

TEST_CASE("Finite-difference barrier rebates preserve payment timing and discounting", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto scheme = GENERATE(FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson);
    const bool explicit_scheme = scheme == FiniteDifferenceScheme::explicit_euler;
    const FiniteDifferenceBarrierEngine engine{{.asset_step_count = explicit_scheme ? 80 : 800, .time_step_count = 400, .scheme = scheme, .asset_upper_boundary = 400.0}};
    for (const double rate : {-0.04, 0.04}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, 0.01, 0.2), 100.0, start);
        for (const auto kind : {BarrierType::down_and_out, BarrierType::up_and_out}) {
            for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
                CAPTURE(scheme, rate, kind, timing);
                const auto option = *make_barrier_option(
                    {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = kind == BarrierType::up_and_out ? 120.0 : 80.0, .barrier_type = kind, .rebate = 5.0, .rebate_timing = timing});
                const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                const auto expected = AnalyticBarrierEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                REQUIRE(actual);
                REQUIRE(expected);
                CHECK(actual->price() == *engine.price(option, context));
                CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(expected->price(), explicit_scheme ? 0.1 : 0.01));
                CHECK_THAT(*actual->require(Greek::delta), Catch::Matchers::WithinAbs(*expected->require(Greek::delta), explicit_scheme ? 0.01 : 0.003));
                CHECK_THAT(*actual->require(Greek::gamma), Catch::Matchers::WithinAbs(*expected->require(Greek::gamma), explicit_scheme ? 0.002 : 0.0005));
            }
        }
    }
}

TEST_CASE("Finite-difference knock-in prices preserve small positive values", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto start = Date{std::chrono::year{2025} / 1 / 1};
    const auto end = Date{std::chrono::year{2026} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const bool call = type == OptionType::call;
        const auto option = *make_barrier_option(
            {.option_type = type, .strike = call ? 150.0 : 50.0, .effective_date = start, .expiry_date = end, .barrier_level = call ? 70.0 : 130.0, .barrier_type = call ? BarrierType::down_and_in : BarrierType::up_and_in});
        const auto expected = AnalyticBarrierEngine{}.price(option, context);
        REQUIRE(expected);
        for (const int steps : {200, 800}) {
            CAPTURE(type, steps);
            const FiniteDifferenceBarrierEngine engine{{steps, 200}};
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK(*price > 0.0);
            CHECK_THAT(*price, Catch::Matchers::WithinAbs(*expected, 5e-7));
            const auto joint = engine.price_with_greeks(option, context, {Greek::delta});
            REQUIRE(joint);
            CHECK(joint->price() == *price);
        }
        const auto coarse = FiniteDifferenceBarrierEngine{{3, 200}}.price(option, context);
        REQUIRE(coarse);
        CHECK(*coarse >= 0.0);
    }
}

TEST_CASE("Finite-difference barriers preserve long-expiry volatility tails")
{
    const auto start = kiyosi::Date{std::chrono::year{2025} / 1 / 1};
    const auto end = start + std::chrono::days{1825};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.0, 0.8), 100.0, start, kiyosi::all_days_calendar());
    for (const auto mode : {kiyosi::ObservationMode::continuous, kiyosi::ObservationMode::scheduled}) {
        const auto option = *kiyosi::make_barrier_option(
            {.option_type = kiyosi::OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 0.01, .barrier_type = kiyosi::BarrierType::down_and_out, .observation_mode = mode, .observation_dates = mode == kiyosi::ObservationMode::scheduled ? std::vector<kiyosi::Date>{end} : std::vector<kiyosi::Date>{}});
        const auto expected = kiyosi::AnalyticBarrierEngine{}.price(option, context);
        REQUIRE(expected);
        for (const auto settings : {kiyosi::FiniteDifferenceSettings{}, kiyosi::FiniteDifferenceSettings{800, 1000}}) {
            CAPTURE(mode, settings.asset_step_count, settings.time_step_count);
            const auto price = kiyosi::FiniteDifferenceBarrierEngine{settings}.price(option, context);
            REQUIRE(price);
            CHECK_THAT(*price, Catch::Matchers::WithinAbs(*expected, 0.02));
        }
    }
}

TEST_CASE("Finite-difference automatic domains include distant barriers")
{
    const auto start = kiyosi::Date{std::chrono::year{2025} / 1 / 1};
    const auto end = start + std::chrono::days{365};
    const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.03, 0.02, 0.2), 100.0, start);
    const auto option = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                      .strike = 100.0,
                                                      .effective_date = start,
                                                      .expiry_date = end,
                                                      .barrier_level = 500.0,
                                                      .barrier_type = kiyosi::BarrierType::up_and_out});
    const kiyosi::FiniteDifferenceBarrierEngine finite{800, 1000};
    const auto price = finite.price(option, context);
    REQUIRE(price);
    CHECK_THAT(*price, Catch::Matchers::WithinAbs(*kiyosi::AnalyticBarrierEngine{}.price(option, context), 0.03));
    const auto default_price = kiyosi::FiniteDifferenceBarrierEngine{}.price(option, context);
    REQUIRE(default_price);
    CHECK_THAT(*default_price, Catch::Matchers::WithinAbs(*kiyosi::AnalyticBarrierEngine{}.price(option, context), 0.05));
    const auto joint = finite.price_with_greeks(option, context, {kiyosi::Greek::gamma});
    REQUIRE(joint);
    CHECK(joint->price() == *price);
    const auto clipped = kiyosi::FiniteDifferenceBarrierEngine{{800, 1000, kiyosi::FiniteDifferenceScheme::crank_nicolson, 400.0}}.price(option, context);
    REQUIRE_FALSE(clipped);
    CHECK(clipped.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("QuantLib continuous barrier portfolios validate prices and numerical Greeks")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    std::map<std::string, int> generated, wrappers;
    for (const auto& fixture : cases) {
        const auto& inputs = fixture.inputs;
        if (fixture.instrument != "BarrierOption") continue;
        INFO("case=" << fixture.case_id);
        REQUIRE(inputs.at("owner") == "QuantLib");
        REQUIRE(fixture.provenance.reference_kind == "analytic");
        REQUIRE(fixture.provenance.source_symbol == "QuantLib.AnalyticBarrierEngine+QuantLib.DiscountingBondEngine+QuantLib.AnalyticEuropeanEngine");
        REQUIRE(inputs.at("monitoring") == "continuous");
        const auto number = [&](const std::string& key) { return fixture_number(fixture, key); };
        const auto date = [&](const std::string& key) { return fixture_date(fixture, key); };
        REQUIRE(barrier_kinds.contains(inputs.at("BarrierType")));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        REQUIRE((inputs.at("settlement") == "at_hit" || inputs.at("settlement") == "at_expiry"));
        const auto option = kiyosi::make_barrier_option({.option_type = inputs.at("option") == "call" ? kiyosi::OptionType::call : kiyosi::OptionType::put,
                                                         .strike = number("strike"),
                                                         .effective_date = date("effective_date"),
                                                         .expiry_date = date("expiry_date"),
                                                         .barrier_level = number("barrier"),
                                                         .barrier_type = barrier_kinds.at(inputs.at("BarrierType")),
                                                         .rebate = number("rebate"),
                                                         .rebate_timing = inputs.at("settlement") == "at_hit" ? kiyosi::RebateTiming::at_hit
                                                                                                              : kiyosi::RebateTiming::at_expiry,
                                                         .touch_state = kiyosi::BarrierTouchState::untouched});
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(
            *parameters, number("spot"), date("valuation"), kiyosi::weekdays_calendar());
        REQUIRE(context.has_value());
        const auto check = [&](const auto& engine) {
            const auto native = engine.price(*option, *context);
            check_price(fixture, native);
            ++generated[fixture.engine];
            REQUIRE((inputs.at("wrapper") == "true" || inputs.at("wrapper") == "false"));
            const bool boundary = (date("expiry_date") - date("valuation")).count() <= 2;
            if (boundary) REQUIRE(inputs.at("wrapper") == "false");
            for (const auto& [name, value] : fixture.outputs)
                REQUIRE((name == "price" || measures.contains(name)));
            if (inputs.at("wrapper") == "false") return;
            ++wrappers[fixture.engine];
            const kiyosi::NumericalShiftSettings shifts{number("spot_shift"), number("volatility_shift"),
                                                        number("rate_shift"), static_cast<int>(number("time_shift_days"))};
            // Three nested spot shifts are used by speed; keep every stencil in the same hit state.
            REQUIRE(std::abs(number("spot") - number("barrier")) > 3 * shifts.spot_shift);
            const auto numerical = kiyosi::calculate_numerical_greeks(engine, *option, *context, shifts);
            kiyosi::test::check_numerical_result(fixture, numerical);
        };
        if (fixture.engine == "AnalyticBarrierEngine") check(kiyosi::AnalyticBarrierEngine{});
        else {
            REQUIRE(fixture.engine == "FiniteDifferenceBarrierEngine");
            REQUIRE(inputs.at("scheme") == "crank_nicolson");
            check(kiyosi::FiniteDifferenceBarrierEngine{
                {static_cast<int>(number("asset_step_count")), static_cast<int>(number("time_step_count")),
                 kiyosi::FiniteDifferenceScheme::crank_nicolson, number("asset_upper_boundary")}});
        }
    }
    REQUIRE(generated.size() == 2);
    CHECK(generated["AnalyticBarrierEngine"] == 60);
    CHECK(generated["FiniteDifferenceBarrierEngine"] == 60);
    CHECK(wrappers["AnalyticBarrierEngine"] == 48);
    CHECK(wrappers["FiniteDifferenceBarrierEngine"] == 12);
}
