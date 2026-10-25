#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <atomic>
#include <cmath>
#include <future>
#include <memory>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>
#include "support/numerical.hpp"

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
    },
                                                 252);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(end) - std::chrono::hours{12}, calendar);
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = is_up_barrier(kind) ? 120.0 : 80.0, .barrier_type = kind, .observation_mode = ObservationMode::scheduled, .observation_dates = dates, .touch_state = BarrierTouchState::untouched});
    const auto engine = make_finite_difference_barrier_engine({80, 1, scheme}).value();
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
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar and is returned by value.
        return *make_pricing_context(*make_bsm_parameters(rate, 0.01, volatility), context.spot_price(), context.valuation_time(), calendar);
    };
    const auto fixed = make_finite_difference_barrier_engine({80, 1, scheme, 4.0 * context.spot_price()}).value();
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
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {start, end}, .touch_state = BarrierTouchState::untouched});
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(end) - std::chrono::hours{12}, all_days_calendar());
    const auto engine = make_finite_difference_barrier_engine({80, 1, FiniteDifferenceScheme::explicit_euler}).value();
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

TEST_CASE("Scheduled finite-difference knock-in layers preserve rebate parity", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, all_days_calendar());
    for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson}) {
        const FiniteDifferenceSettings settings{80, 400, scheme, 400.0};
        const auto engine = make_finite_difference_barrier_engine(settings).value();
        for (const auto type : {OptionType::call, OptionType::put}) {
            const auto vanilla = make_finite_difference_vanilla_engine(settings).value().price_with_greeks(
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
                            CHECK_THAT(*in->require(greek) + *out->require(greek), Catch::Matchers::WithinAbs(*vanilla->require(greek), 1e-11));
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
    const auto engine = make_finite_difference_barrier_engine({800, 400}).value();
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
    const auto engine = make_finite_difference_barrier_engine({800, 400}).value();
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
    const auto engine = make_finite_difference_barrier_engine({.asset_step_count = explicit_scheme ? 80 : 800, .time_step_count = 400, .scheme = scheme, .asset_upper_boundary = 400.0}).value();
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
            const auto engine = make_finite_difference_barrier_engine({steps, 200}).value();
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK(*price > 0.0);
            CHECK_THAT(*price, Catch::Matchers::WithinAbs(*expected, 5e-7));
            const auto joint = engine.price_with_greeks(option, context, {Greek::delta});
            REQUIRE(joint);
            CHECK(joint->price() == *price);
        }
        const auto coarse = make_finite_difference_barrier_engine({3, 200}).value().price(option, context);
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
            const auto price = kiyosi::make_finite_difference_barrier_engine(settings).value().price(option, context);
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
    const auto finite = kiyosi::make_finite_difference_barrier_engine({800, 1000}).value();
    const auto price = finite.price(option, context);
    REQUIRE(price);
    CHECK_THAT(*price, Catch::Matchers::WithinAbs(*kiyosi::AnalyticBarrierEngine{}.price(option, context), 0.03));
    const auto default_price = kiyosi::FiniteDifferenceBarrierEngine{}.price(option, context);
    REQUIRE(default_price);
    CHECK_THAT(*default_price, Catch::Matchers::WithinAbs(*kiyosi::AnalyticBarrierEngine{}.price(option, context), 0.05));
    const auto joint = finite.price_with_greeks(option, context, {kiyosi::Greek::gamma});
    REQUIRE(joint);
    CHECK(joint->price() == *price);
    const auto clipped = kiyosi::make_finite_difference_barrier_engine({800, 1000, kiyosi::FiniteDifferenceScheme::crank_nicolson, 400.0}).value().price(option, context);
    REQUIRE_FALSE(clipped);
    CHECK(clipped.error().category == kiyosi::ErrorCategory::invalid_parameter);
}
