#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <concepts>
#include <cstdint>
#include <future>
#include <limits>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "pricing/detail/numerical_greeks.hpp"
#include "support/common.hpp"

namespace {

using namespace kiyosi;
using kiyosi::test::day;

MonteCarloSettings vanilla_settings(int paths = 1024, int threads = 1)
{
    return {paths, 7, 42, MonteCarloBackend::cpu, threads, MonteCarloSampling::sobol};
}

TradingDayMonteCarloSettings trading_settings(int paths = 1024, int threads = 1)
{
    return {paths, 42, MonteCarloBackend::cpu, threads, MonteCarloSampling::sobol};
}

template <typename Check>
void for_each_sobol_product(const Check& check, int paths = 1024, int threads = 1)
{
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    const auto vanilla = make_monte_carlo_vanilla_engine(vanilla_settings(paths, threads)).value();
    check(vanilla, *make_european_option(OptionType::call, 100.0, start, end));
    check(vanilla, *make_american_option(OptionType::put, 100.0, start, end));
    check(make_monte_carlo_accumulator_engine(trading_settings(paths, threads)).value(),
          *make_accumulator({.strike = 100.0, .knock_out_level = 101.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end}));
    const auto autocallable = make_monte_carlo_autocallable_engine(trading_settings(paths, threads)).value();
    const std::vector<AutocallableObservation> observations{
        {day(2025, 1, 3), 101.0, 0.08}, {day(2025, 1, 8), 100.0, 0.08}, {end, 99.0, 0.08}};
    check(autocallable, *make_binary_snowball_option({.maturity_coupon_rate = 0.06,
                                                      .barrier_state = AutocallableBarrierState::none,
                                                      .effective_date = start,
                                                      .expiry_date = end,
                                                      .observations = observations}));
    for (const auto mode : {KnockInObservationMode::at_expiry, KnockInObservationMode::every_trading_day}) {
        check(autocallable, *make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 99.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations}));
        check(autocallable, *make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations}));
        check(autocallable, *make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 1, 3), 101.0, 0.0, 99.0}, {day(2025, 1, 8), 100.0, 0.0, 99.0}, {end, 99.0, 0.0, 99.0}}}));
    }
}

} // namespace

TEST_CASE("Sobol sampling leaves pseudorandom defaults and prices unchanged", "[sobol][engine-creation]")
{
    CHECK(MonteCarloSettings{}.sampling == MonteCarloSampling::pseudo_random);
    CHECK(TradingDayMonteCarloSettings{}.sampling == MonteCarloSampling::pseudo_random);
    CHECK(MonteCarloSettings{}.path_count == 100'000);
    CHECK(TradingDayMonteCarloSettings{}.path_count == 20'000);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               day(2025, 1, 1), all_days_calendar());
    for_each_sobol_product([&](const auto& engine, const auto& option) {
        auto settings = engine.settings();
        settings.path_count = 1023;
        settings.sampling = MonteCarloSampling::pseudo_random;
        const auto explicit_engine = make_engine(settings, std::type_identity<std::decay_t<decltype(engine)>>{});
        REQUIRE(explicit_engine);
        auto defaults = decltype(settings){};
        defaults.path_count = settings.path_count;
        defaults.seed = settings.seed;
        defaults.thread_count = settings.thread_count;
        if constexpr (std::same_as<decltype(settings), MonteCarloSettings>) defaults.step_count = settings.step_count;
        const auto default_engine = make_engine(defaults, std::type_identity<std::decay_t<decltype(engine)>>{});
        REQUIRE(default_engine);
        const auto expected = default_engine->price(option, context);
        const auto actual = explicit_engine->price(option, context);
        REQUIRE(expected);
        REQUIRE(actual);
        CHECK(*actual == *expected);
    });
}

TEST_CASE("Sobol factories accept powers of two and reject invalid counts and sampling choices", "[sobol][engine-creation]")
{
    const auto check_factories = [](int paths, MonteCarloSampling sampling, bool valid) {
        auto vanilla = vanilla_settings(paths);
        auto trading = trading_settings(paths);
        vanilla.sampling = sampling;
        trading.sampling = sampling;
        const auto check = [valid](const auto& result) {
            REQUIRE(result.has_value() == valid);
            if (!valid) CHECK(result.error().category == ErrorCategory::invalid_parameter);
            else CHECK(result->settings().sampling == MonteCarloSampling::sobol);
        };
        check(make_monte_carlo_vanilla_engine(vanilla));
        check(make_monte_carlo_accumulator_engine(trading));
        check(make_monte_carlo_autocallable_engine(trading));
    };
    for (const int paths : {1, 2, 1024, 8'388'608}) {
        CAPTURE(paths);
        check_factories(paths, MonteCarloSampling::sobol, true);
    }
    for (const int paths : {-1, 0, 3, 1023, 1025, 20'000, 100'000, 8'388'607, 8'388'609, 16'777'216}) {
        CAPTURE(paths);
        check_factories(paths, MonteCarloSampling::sobol, false);
    }
    check_factories(1024, static_cast<MonteCarloSampling>(255), false);
}

TEST_CASE("Sobol CUDA combinations fail during engine creation with a core unsupported error", "[sobol][engine-creation]")
{
    auto vanilla = vanilla_settings();
    auto trading = trading_settings();
    vanilla.backend = MonteCarloBackend::cuda;
    trading.backend = MonteCarloBackend::cuda;
    vanilla.thread_count = 0;
    trading.thread_count = 0;
    const auto check = [](const auto& result) {
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::unsupported_operation);
    };
    check(make_monte_carlo_vanilla_engine(vanilla));
    check(make_monte_carlo_accumulator_engine(trading));
    check(make_monte_carlo_autocallable_engine(trading));
}

TEST_CASE("Sobol prices and Greeks reproduce exactly across CPU thread counts and concurrent calls", "[sobol][mc-parallel]")
{
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(day(2025, 1, 1)) + std::chrono::hours{12}, all_days_calendar());
    for_each_sobol_product([&](const auto& serial, const auto& option) {
        const auto expected = serial.price_with_greeks(option, context, true);
        REQUIRE(expected);
        CHECK(expected->price() == *serial.price(option, context));
        for (const int threads : {0, 2, 4}) {
            CAPTURE(threads);
            auto settings = serial.settings();
            settings.thread_count = threads;
            const auto engine = make_engine(settings, std::type_identity<std::decay_t<decltype(serial)>>{}).value();
            const auto actual = engine.price_with_greeks(option, context, true);
            REQUIRE(actual);
            CHECK(actual->price() == expected->price());
            CHECK(actual->values_view() == expected->values_view());
        }
        auto pending = std::async(std::launch::async, [&] { return serial.price_with_greeks(option, context, true); });
        const auto foreground = serial.price_with_greeks(option, context, true);
        const auto background = pending.get();
        REQUIRE(foreground);
        REQUIRE(background);
        CHECK(foreground->price() == expected->price());
        CHECK(background->price() == expected->price());
        CHECK(foreground->values_view() == expected->values_view());
        CHECK(background->values_view() == expected->values_view());
    });
}

TEST_CASE("Sobol joint Greeks retain the scalar common-sample stencils for every product", "[sobol][mc-shared-samples]")
{
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(day(2025, 1, 1)) + std::chrono::hours{12}, all_days_calendar());
    const NumericalShiftSettings shifts{.spot_shift = 1.0, .volatility_shift = 0.01, .rate_shift = 0.001};
    for_each_sobol_product([&](const auto& engine, const auto& option) {
        const auto scalar = detail::price_with_greeks(engine, option, context, true, shifts);
        REQUIRE(scalar);
        for (const GreeksRequest request : {GreeksRequest{true}, GreeksRequest{Greek::delta, Greek::gamma, Greek::rho},
                                            GreeksRequest{Greek::vega, Greek::vanna, Greek::zomma}}) {
            const auto joint = engine.price_with_greeks(option, context, request, shifts);
            REQUIRE(joint);
            CHECK(joint->price() == scalar->price());
            const auto selected = scalar->selected(request);
            for (std::size_t index = 0; index < greek_count; ++index) {
                CAPTURE(index);
                const auto expected = selected.values_view()[index];
                const auto actual = joint->values_view()[index];
                REQUIRE(actual.has_value() == expected.has_value());
                if (expected) CHECK(*actual == Catch::Approx(*expected).margin(1e-10));
            }
        }
    });
}

TEST_CASE("Sobol implied volatility and coupon requests reuse their seeded estimator", "[sobol][mc-shared-samples]")
{
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    for_each_sobol_product([&](const auto& engine, const auto& option) {
        const auto price = engine.price(option, context);
        REQUIRE(price);
        const ImpliedVolatilitySettings solve{.lower_bound = 0.15, .upper_bound = 0.25, .price_tolerance = 1e-10, .parameter_tolerance = 1e-12};
        const auto recovered = implied_volatility(engine, option, context, *price, solve);
        REQUIRE(recovered);
        CHECK(*recovered >= solve.lower_bound);
        CHECK(*recovered <= solve.upper_bound);
        const auto shifted = *make_pricing_context(
            *make_bsm_parameters(context.model_parameters().risk_free_rate(), context.model_parameters().dividend_yield(), *recovered),
            context.spot_price(), context.valuation_time(), context.calendar());
        const auto repriced = engine.price(option, shifted);
        REQUIRE(repriced);
        CHECK_THAT(*repriced, Catch::Matchers::WithinAbs(*price, 1e-8));
        const auto repeated = implied_volatility(engine, option, context, *price, solve);
        REQUIRE(repeated);
        CHECK(*repeated == *recovered);
    });
    const auto snowball = *make_snowball_option({.maturity_coupon_rate = 0.073, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 1, 7), 101.0, 0.073}, {end, 101.0, 0.073}}});
    const auto engine = make_monte_carlo_autocallable_engine(trading_settings()).value();
    const auto price = engine.price(snowball, context);
    REQUIRE(price);
    const auto coupon = implied_coupon(engine, snowball, context, *price,
                                       CouponQuoteConvention::shift_maturity_coupon,
                                       {.lower_bound = 0.0, .upper_bound = 0.2, .price_tolerance = 1e-13});
    REQUIRE(coupon);
    CHECK(*coupon == Catch::Approx(0.073).margin(1e-9));
}

TEST_CASE("Sobol European prices and Greeks agree with analytical references", "[sobol][pricing-accuracy]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto engine = make_monte_carlo_vanilla_engine(vanilla_settings(65'536)).value();
    // Tolerances include finite-difference bump bias and finite-sample error in each Greek's units.
    constexpr std::array tolerances{
        std::pair{Greek::delta, 2e-4}, std::pair{Greek::gamma, 1e-5},
        std::pair{Greek::speed, 5e-6}, std::pair{Greek::vega, 5e-5},
        std::pair{Greek::vanna, 1e-5}, std::pair{Greek::zomma, 5e-6},
        std::pair{Greek::rho, 2e-5}, std::pair{Greek::theta, 2e-5},
        std::pair{Greek::charm, 5e-7}, std::pair{Greek::color, 2e-7}};
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto option = *make_european_option(type, 100.0, start, end);
        const auto reference = AnalyticVanillaEngine{}.price_with_greeks(option, context, true);
        const auto actual = engine.price_with_greeks(option, context, true,
                                                     {.spot_shift = 1.0, .volatility_shift = 0.001, .rate_shift = 0.001});
        REQUIRE(reference);
        REQUIRE(actual);
        CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(reference->price(), 0.002));
        for (const auto& [greek, tolerance] : tolerances) {
            CAPTURE(type, greek);
            CHECK_THAT(test::greek_value(*actual, greek), Catch::Matchers::WithinAbs(test::greek_value(*reference, greek), tolerance));
        }
        auto different_steps = engine.settings();
        different_steps.step_count = 10'000;
        const auto terminal_only = make_monte_carlo_vanilla_engine(different_steps).value().price(option, context);
        REQUIRE(terminal_only);
        CHECK(*terminal_only == actual->price());
    }
}

TEST_CASE("One Sobol point prices exactly one European path without antithetic pairing", "[sobol][pricing-accuracy]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto option = *make_european_option(OptionType::call, 90.0, start, end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto engine = make_monte_carlo_vanilla_engine(vanilla_settings(1)).value();
    // Independent SciPy ndtri reference for the seed-42 first midpoint Sobol coordinate.
    constexpr double first_normal = -0.10364940758940244;
    const double terminal = 100.0 * std::exp(0.01 + 0.2 * first_normal);
    const double expected = std::exp(-0.05) * (terminal - 90.0);
    const auto actual = engine.price(option, context);
    REQUIRE(actual);
    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(expected, 1e-11));
    CHECK(engine.settings().path_count == 1);
}

TEST_CASE("Sobol American prices agree with independently refined finite differences", "[sobol][pricing-accuracy]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    auto settings = vanilla_settings(32'768);
    settings.step_count = 50;
    const auto engine = make_monte_carlo_vanilla_engine(settings).value();
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto option = *make_american_option(type, 100.0, start, end);
        const auto reference = make_finite_difference_vanilla_engine({1200, 1200}).value().price(option, context);
        const auto actual = engine.price(option, context);
        REQUIRE(reference);
        REQUIRE(actual);
        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*reference, 0.2));
    }
}

TEST_CASE("Sobol path-dependent prices agree with independent quadrature", "[sobol][pricing-accuracy]")
{
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(day(2025, 1, 1)) + std::chrono::hours{12}, all_days_calendar());
    for_each_sobol_product([&](const auto& engine, const auto& option) {
        if constexpr (std::same_as<std::decay_t<decltype(engine)>, MonteCarloAccumulatorEngine>) {
            const auto reference = QuadratureAccumulatorEngine{}.price(option, context);
            const auto actual = engine.price(option, context);
            REQUIRE(reference);
            REQUIRE(actual);
            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*reference, 0.25));
        } else if constexpr (std::same_as<std::decay_t<decltype(engine)>, MonteCarloAutocallableEngine>) {
            const auto reference = QuadratureAutocallableEngine{}.price(option, context);
            const auto actual = engine.price(option, context);
            REQUIRE(reference);
            REQUIRE(actual);
            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*reference, 0.002));
        }
    },
                           32'768);
}

TEST_CASE("Sobol changes randomization with the seed and retains unseeded request settings", "[sobol][mc-shared-samples]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto option = *make_european_option(OptionType::call, 100.0, start, end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    auto settings = vanilla_settings(16);
    const auto first = make_monte_carlo_vanilla_engine(settings).value().price(option, context);
    settings.seed = std::numeric_limits<std::uint64_t>::max();
    const auto second = make_monte_carlo_vanilla_engine(settings).value().price(option, context);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(*first != *second);
    settings.seed = std::nullopt;
    const auto engine = make_monte_carlo_vanilla_engine(settings).value();
    const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::vega});
    REQUIRE(actual);
    CHECK(actual->all_finite());
    CHECK_FALSE(engine.settings().seed);
}

TEST_CASE("Sobol rejects excessive trading-day dimensions without poisoning the engine", "[sobol][engine-validation]")
{
    const auto start = day(2025, 1, 1), end = start + std::chrono::days{21'203};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0,
                                               start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    const auto accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 1000.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    const auto snowball = *make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}});
    const auto accumulator_engine = make_monte_carlo_accumulator_engine(trading_settings(1)).value();
    const auto autocallable_engine = make_monte_carlo_autocallable_engine(trading_settings(1)).value();
    const auto short_end = start + std::chrono::days{3};
    const auto short_accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 1000.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = short_end});
    const auto short_snowball = *make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = short_end, .observations = {{short_end, 110.0, 0.08}}});
    const auto check = [&](const auto& engine, const auto& option, const auto& shorter) {
        const auto expected = engine.price(shorter, context);
        REQUIRE(expected);
        const auto too_many = engine.price(option, context);
        REQUIRE_FALSE(too_many);
        CHECK(too_many.error().category == ErrorCategory::invalid_parameter);
        const auto recovered = engine.price(shorter, context);
        REQUIRE(recovered);
        CHECK(*recovered == *expected);
    };
    check(accumulator_engine, accumulator, short_accumulator);
    check(autocallable_engine, snowball, short_snowball);

    const auto maximum_end = start + std::chrono::days{21'201};
    const auto maximum_accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 1000.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = maximum_end});
    const auto maximum_snowball = *make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = maximum_end, .observations = {{maximum_end, 110.0, 0.08}}});
    REQUIRE(accumulator_engine.price(maximum_accumulator, context));
    REQUIRE(autocallable_engine.price(maximum_snowball, context));
    const auto sparse_observations = *make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}});
    REQUIRE(autocallable_engine.price(sparse_observations, context));
}
