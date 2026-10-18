#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <catch2/generators/catch_generators.hpp>

#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <future>
#include <limits>
#include <memory>
#include <optional>
#include <random>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"
#include "support/numerical.hpp"
#include "pricing/engines/monte_carlo_random.hpp"
#include "pricing/engines/monte_carlo_autocallable.hpp"

namespace {

using kiyosi::test::day;

TEST_CASE("Autocallable automatic domains converge to volatile terminal portfolios", "[autocallable-domain]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 2.0), 100.0, start);
    const auto digital = *make_cash_or_nothing_option(OptionType::call, 100.0, 0.1, start, end);
    const double expected_binary = 1.0 + *AnalyticDigitalEngine{}.price(digital, context);
    const auto binary = *make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}});
    for (const int steps : {200, 2000}) {
        const auto engine = *make_finite_difference_autocallable_engine({steps, 2000});
        const auto result = engine.price(binary, context);
        REQUIRE(result);
        CAPTURE(steps);
        CHECK(*result == Catch::Approx(expected_binary).margin(0.0003));
    }
    const auto engine = *make_finite_difference_autocallable_engine({2000, 2000});
    const auto put = [&](double strike) {
        return *AnalyticVanillaEngine{}.price(*make_european_option(OptionType::put, strike, start, end), context);
    };
    const double expected_downside = expected_binary - (put(100.0) - put(60.0)) / 100.0;
    const auto snowball = *make_snowball_option({.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 100.0, .upper_strike = 100.0, .lower_strike = 60.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}});
    const auto phoenix = *make_phoenix_option({.coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 100.0, .upper_strike = 100.0, .lower_strike = 60.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.0, 100.0}}});
    const auto ternary = *make_ternary_snowball_option({.maturity_coupon_rate = 0.1, .knock_in_level = 100.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}});
    CHECK(*engine.price(snowball, context) == Catch::Approx(expected_downside).margin(0.0003));
    CHECK(*engine.price(phoenix, context) == Catch::Approx(expected_downside).margin(0.0003));
    CHECK(*engine.price(ternary, context) == Catch::Approx(expected_binary).margin(0.0003));
    const auto actual = engine.price_with_greeks(binary, context, {Greek::delta, Greek::gamma, Greek::vega, Greek::rho});
    const auto expected = AnalyticDigitalEngine{}.price_with_greeks(digital, context, {Greek::delta, Greek::gamma, Greek::vega, Greek::rho});
    REQUIRE(actual);
    REQUIRE(expected);
    for (const auto greek : {Greek::delta, Greek::gamma, Greek::vega, Greek::rho}) {
        const double reference = test::greek_value(*expected, greek) - (greek == Greek::rho ? 0.01 : 0.0);
        CHECK(test::greek_value(*actual, greek) == Catch::Approx(reference).margin(std::abs(reference) * 0.02 + 1e-8));
    }
    const auto fixed = *make_finite_difference_autocallable_engine({2000, 2000, FiniteDifferenceScheme::crank_nicolson, 1e6});
    const auto high = *make_pricing_context(context.model_parameters(), 100.1, start);
    const auto low = *make_pricing_context(context.model_parameters(), 99.9, start);
    const double delta = (*fixed.price(binary, high) - *fixed.price(binary, low)) / 0.2;
    const auto native_delta = fixed.price_with_greeks(binary, context, {Greek::delta});
    REQUIRE(native_delta);
    CHECK(test::greek_value(*native_delta, Greek::delta) == Catch::Approx(delta).margin(std::abs(delta) * 0.02));
    const auto quote = *engine.price(binary, context);
    const auto volatility = implied_volatility(engine, binary, context, quote,
                                               {.lower_bound = 1.5, .upper_bound = 2.5, .price_tolerance = 1e-10, .parameter_tolerance = 1e-10});
    REQUIRE(volatility);
    const auto recovered = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, *volatility), 100.0, start);
    CHECK(*engine.price(binary, recovered) == Catch::Approx(quote).margin(1e-8));
    const auto explicit_engine = *make_finite_difference_autocallable_engine({200, 2000, FiniteDifferenceScheme::explicit_euler});
    const auto explicit_price = explicit_engine.price(binary, context);
    REQUIRE(explicit_price);
    CHECK(*explicit_price == Catch::Approx(expected_binary).margin(0.0003));
}

TEST_CASE("Autocallable engines reuse one instance across every product", "[autocallable-engine]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto market = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 120.0, end, all_days_calendar());
    const auto snowball = *make_snowball_option({.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}});
    const auto binary = *make_binary_snowball_option({.maturity_coupon_rate = 0.05,
                                                      .barrier_state = AutocallableBarrierState::none,
                                                      .effective_date = start,
                                                      .expiry_date = end,
                                                      .observations = {{end, 110.0, 0.08}}});
    const auto ternary = *make_ternary_snowball_option({.maturity_coupon_rate = 0.05, .knock_in_level = 80.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}});
    const auto phoenix = *make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.0, 90.0}}});
    const auto finite_difference = make_finite_difference_autocallable_engine({32, 2}).value();
    const auto monte_carlo = make_monte_carlo_autocallable_engine({32, 7}).value();
    const auto check = [&](const auto& note) {
        for (const auto& price : {finite_difference.price(note, market), monte_carlo.price(note, market)}) {
            REQUIRE(price);
            CHECK(*price == 1.08);
        }
        for (const auto& result : {finite_difference.price_with_greeks(note, market, {Greek::delta}),
                                   monte_carlo.price_with_greeks(note, market, {Greek::delta})}) {
            REQUIRE(result);
            CHECK(result->price() == 1.08);
            CHECK_FALSE(result->has(Greek::delta));
        }
    };
    check(snowball);
    check(binary);
    check(ternary);
    check(phoenix);
}

TEST_CASE("Implied volatility request preparation runs once before seeded trials", "[implied-request-preparation]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto option = *make_european_option(OptionType::call, 100.0, start, end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    const auto engine = make_monte_carlo_vanilla_engine(MonteCarloSettings{.path_count = 31, .step_count = 3}).value();
    int preparations = 0, trials = 0;
    std::optional<std::uint64_t> seed;
    const auto price = [&](const auto& seeded, const auto&, const auto& market) -> Result<double> {
        CHECK(preparations == 1);
        CHECK(seeded.settings().seed == seed);
        ++trials;
        return market.model_parameters().volatility();
    };
    const auto prepare = [&](const auto& seeded, ImpliedVolatilitySettings settings) -> Result<ImpliedVolatilitySettings> {
        ++preparations;
        seed = seeded.settings().seed;
        CHECK(seed.has_value());
        return settings;
    };
    const auto invalid = detail::solve_implied_volatility(engine, option, context, 0.3,
                                                          {.lower_bound = 0.0}, price, prepare);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    CHECK(preparations == 0);
    CHECK(trials == 0);
    const auto solved = detail::solve_implied_volatility(engine, option, context, 0.3, {}, price, prepare);
    REQUIRE(solved);
    CHECK(*solved == Catch::Approx(0.3).margin(1e-8));
    CHECK(preparations == 1);
    CHECK(trials >= 3);
    trials = 0;
    const auto rejected = detail::solve_implied_volatility(engine, option, context, 0.3, {}, price,
                                                           [](const auto&, ImpliedVolatilitySettings) -> Result<ImpliedVolatilitySettings> {
                                                               return std::unexpected(Error{ErrorCategory::unsupported_operation, "request preparation rejected"});
                                                           });
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::unsupported_operation);
    CHECK(trials == 0);
}

TEST_CASE("Autocallable path observations preserve event boundaries and coupon ordering", "[autocallable-path]")
{
    using namespace kiyosi::detail;
    AutocallableProgram program{1.0, 100.0, 100.0, 60.0, 80.0, 0.06, 0.0,
                                AutocallableTerminalKind::downside_if_knocked_in, true, true, true};
    const MonteCarloDiscount discount{0.5, std::log(0.5)};
    AutocallablePathState state{0.1, false};
    AutocallableEvent event{110.0, 0.08, 90.0, true, false};
    double payoff = -1.0;
    CHECK_FALSE(observe_autocallable_path(program, event, discount, 79.0, state, payoff));
    CHECK(state.knocked_in);
    CHECK(state.coupons == 0.1);
    CHECK(payoff == -1.0);
    event.active = true;
    CHECK_FALSE(observe_autocallable_path(program, event, discount, 89.0, state, payoff));
    CHECK(state.coupons == 0.1);
    CHECK_FALSE(observe_autocallable_path(program, event, discount, 90.0, state, payoff));
    CHECK(state.coupons == Catch::Approx(0.14));
    CHECK(observe_autocallable_path(program, event, discount, 110.0, state, payoff));
    CHECK(payoff == Catch::Approx(0.68));
    CHECK(state.coupons == Catch::Approx(0.14));

    program.daily_knock_in = false;
    program.carries_observation_coupon = false;
    state = {0.0, false};
    CHECK_FALSE(observe_autocallable_path(program, event, discount, 79.0, state, payoff));
    CHECK_FALSE(state.knocked_in);
    CHECK_FALSE(observe_autocallable_path(program, event, discount, 90.0, state, payoff));
    CHECK(state.coupons == 0.0);
    CHECK(settle_autocallable_path(program, discount, 80.0, state) == Catch::Approx(0.53));
    CHECK(settle_autocallable_path(program, discount, 79.0, state) == Catch::Approx(0.395));
}

TEST_CASE("Unavailable speed retains structured Monte Carlo spot batching", "[autocallable-shared-samples]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    int queries = 0;
    const auto calendar = *make_trading_calendar([&](Date) { ++queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.8), 100.0,
                                               start_of_day(start) + std::chrono::hours{12}, calendar);
    const NumericalShiftSettings shifts{.spot_shift = 60.0};
    const auto check = [&](const auto& option, const auto& engine) {
        const auto expected = detail::price_with_greeks(engine, option, context, {Greek::delta, Greek::gamma, Greek::speed}, shifts);
        REQUIRE(expected);
        queries = 0;
        const auto spot = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma}, shifts);
        REQUIRE(spot);
        const int spot_queries = std::exchange(queries, 0);
        const auto mixed = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::speed}, shifts);
        REQUIRE(mixed);
        CHECK(queries == spot_queries);
        CHECK(mixed->price() == expected->price());
        CHECK(mixed->values_view() == expected->values_view());
        CHECK(mixed->values_view() == spot->values_view());
        CHECK(mixed->has(Greek::delta));
        CHECK(mixed->has(Greek::gamma));
        CHECK_FALSE(mixed->has(Greek::speed));
    };
    check(*make_accumulator({.strike = 100.0, .knock_out_level = 110.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end}), make_monte_carlo_accumulator_engine({31, 42}).value());
    check(*make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}}), make_monte_carlo_autocallable_engine({31, 42}).value());
    check(*make_binary_snowball_option({.maturity_coupon_rate = 0.06, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}}), make_monte_carlo_autocallable_engine({31, 42}).value());
    check(*make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 80.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}}), make_monte_carlo_autocallable_engine({31, 42}).value());
    check(*make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.0, 90.0}}}), make_monte_carlo_autocallable_engine({31, 42}).value());
}

TEST_CASE("Autocallable requests validate immutable schedules once", "[autocallable-schedule]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    std::vector<Date> dates;
    for (auto date = start + std::chrono::days{1}; date <= end; date += std::chrono::days{1})
        dates.push_back(date);
    std::atomic<int> queries{};
    std::atomic<bool> allowed{true};
    const auto calendar = *make_trading_calendar([&](Date date) {
        if (date != dates.front()) return true;
        ++queries;
        return allowed.load();
    },
                                                 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(end) - std::chrono::hours{12}, calendar);
    const auto check = [&](const auto& note) {
        const auto check_engine = [&](const auto& engine) {
            const auto expected = [&] {
                if constexpr (std::same_as<decltype(engine.settings()), FiniteDifferenceSettings>)
                    return detail::price_with_greeks(engine, note, context, true, {},
                                                     [&](const auto& scalar, const PricingContext& shifted, GreeksRequest) {
                                                         return scalar.price_with_greeks(note, shifted, {Greek::delta, Greek::gamma, Greek::speed});
                                                     });
                else
                    return detail::price_with_greeks(engine, note, context, true, {});
            }();
            REQUIRE(expected);
            queries = 0;
            const auto actual = engine.price_with_greeks(note, context, true);
            REQUIRE(actual);
            CHECK(actual->price() == expected->price());
            CHECK(actual->values_view() == expected->values_view());
            CHECK(queries == 1);
            queries = 0;
            auto task = std::async(std::launch::async, [&] { return engine.price_with_greeks(note, context, true); });
            const auto concurrent = engine.price_with_greeks(note, context, true);
            const auto first = task.get();
            REQUIRE(first);
            REQUIRE(concurrent);
            CHECK(first->values_view() == actual->values_view());
            CHECK(concurrent->values_view() == actual->values_view());
            CHECK(queries == 2);
            allowed = false;
            const auto rejected = engine.price_with_greeks(note, context, true);
            REQUIRE_FALSE(rejected);
            CHECK(rejected.error().category == ErrorCategory::invalid_date);
            allowed = true;
            queries = 0;
            REQUIRE(engine.price_with_greeks(note, context, true));
            CHECK(queries == 1);
            queries = 0;
            REQUIRE_FALSE(engine.price_with_greeks(note, context, true, {.spot_shift = 0.0}));
            CHECK(queries == 0);
        };
        const auto engine = make_monte_carlo_autocallable_engine({31, 42}).value();
        check_engine(engine);
        check_engine(make_finite_difference_autocallable_engine({80, 1}).value());
        const auto check_solver = [&](const auto& scalar) {
            const auto quote = scalar.price(note, context);
            REQUIRE(quote);
            const ImpliedVolatilitySettings bounds{.lower_bound = 0.1, .upper_bound = 0.4};
            const auto compare = [&](double price, ImpliedVolatilitySettings settings) {
                const auto expected = detail::solve_implied_volatility(scalar, note, context, price, settings,
                                                                       [](const auto& engine, const auto& option, const auto& shifted) {
                                                                           return engine.price(option, shifted);
                                                                       });
                queries = 0;
                const auto solved = implied_volatility(scalar, note, context, price, settings);
                REQUIRE(solved.has_value() == expected.has_value());
                if (expected) CHECK(*solved == *expected);
                else CHECK(solved.error().category == expected.error().category);
                CHECK(queries == 1);
                return solved;
            };
            const auto solved = compare(*quote, bounds);
            REQUIRE(solved);
            (void)compare(1e10, bounds);
            (void)compare(*quote, {.lower_bound = 0.1, .upper_bound = 0.4, .max_iterations = 1});
            queries = 0;
            auto task = std::async(std::launch::async, [&] { return implied_volatility(scalar, note, context, *quote, bounds); });
            const auto concurrent = implied_volatility(scalar, note, context, *quote, bounds);
            const auto first = task.get();
            REQUIRE(first);
            REQUIRE(concurrent);
            CHECK(*first == *solved);
            CHECK(*concurrent == *solved);
            CHECK(queries == 2);
            allowed = false;
            const auto rejected = implied_volatility(scalar, note, context, *quote, bounds);
            REQUIRE_FALSE(rejected);
            CHECK(rejected.error().category == ErrorCategory::invalid_date);
            allowed = true;
            queries = 0;
            const auto recovered = implied_volatility(scalar, note, context, *quote, bounds);
            REQUIRE(recovered);
            CHECK(*recovered == *solved);
            CHECK(queries == 1);
            queries = 0;
            REQUIRE_FALSE(implied_volatility(scalar, note, context, *quote, {.lower_bound = 0.0}));
            CHECK(queries == 0);
        };
        check_solver(engine);
        check_solver(make_finite_difference_autocallable_engine({80, 1}).value());
    };
    const std::vector<double> coupons(dates.size(), 0.08), levels(dates.size(), 110.0);
    check(*make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(dates, levels, coupons)}));
    check(*make_binary_snowball_option({.maturity_coupon_rate = 0.06, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(dates, levels, coupons)}));
    check(*make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 99.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(dates, levels, coupons)}));
    check(*make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(dates, levels, std::nullopt, std::vector<double>(dates.size(), 100.0))}));
}

TEST_CASE("Trading-day implied volatility preserves scalar trials and isolated workspaces", "[implied-workspace]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    const std::vector<Date> observations{day(2025, 1, 3), day(2025, 1, 8), end};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    const auto check = [&](const auto& option, const auto& make_engine) {
        for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda}) {
#if !KIYOSI_HAS_CUDA
            if (backend == MonteCarloBackend::cuda) continue;
#endif
            for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()})
                for (const int paths : {127, 128}) {
                    CAPTURE(backend, seed, paths);
                    const auto engine = make_engine(TradingDayMonteCarloSettings{paths, seed, backend});
                    const auto quote = engine.price(option, context);
                    REQUIRE(quote);
                    const auto compare = [&](double price, ImpliedVolatilitySettings settings) {
                        // The original scalar path creates fresh pricing storage for each trial.
                        const auto expected = detail::solve_implied_volatility(engine, option, context, price, settings,
                                                                               [](const auto& scalar, const auto& instrument, const auto& shifted) {
                                                                                   return detail::numerical_value(scalar, instrument, shifted);
                                                                               });
                        const auto actual = implied_volatility(engine, option, context, price, settings);
                        REQUIRE(actual.has_value() == expected.has_value());
                        if (expected) CHECK(*actual == *expected);
                        else CHECK(actual.error().category == expected.error().category);
                        return actual;
                    };
                    const ImpliedVolatilitySettings bounds{.lower_bound = 0.1, .upper_bound = 0.4};
                    const auto expected = compare(*quote, bounds);
                    REQUIRE(expected);
                    (void)compare(*quote, {});
                    (void)compare(*quote, {.lower_bound = 0.1, .upper_bound = 0.4, .max_iterations = 1});
                    const auto rejected = compare(1e10, bounds);
                    REQUIRE_FALSE(rejected);
                    CHECK(rejected.error().category == ErrorCategory::unbracketed_volatility);
                    (void)compare(*quote, {.lower_bound = 0.0});
                    auto first = std::async(std::launch::async, [&] { return implied_volatility(engine, option, context, *quote, bounds); });
                    auto second = std::async(std::launch::async, [&] { return implied_volatility(engine, option, context, *quote, bounds); });
                    const auto a = first.get(), b = second.get();
                    REQUIRE(a);
                    REQUIRE(b);
                    CHECK(*a == *expected);
                    CHECK(*b == *expected);
                    CHECK(engine.settings().seed == seed);
                }
        }
    };
    check(*make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end}),
          [](auto settings) { return make_monte_carlo_accumulator_engine(settings).value(); });
    for (const auto mode : {KnockInObservationMode::every_trading_day, KnockInObservationMode::at_expiry}) {
        check(*make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}),
              [](auto settings) { return make_monte_carlo_autocallable_engine(settings).value(); });
        check(*make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::nullopt, std::vector<double>{100.0, 99.0, 98.0})}),
              [](auto settings) { return make_monte_carlo_autocallable_engine(settings).value(); });
        check(*make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 99.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}),
              [](auto settings) { return make_monte_carlo_autocallable_engine(settings).value(); });
    }
    check(*make_binary_snowball_option({.maturity_coupon_rate = 0.05, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}),
          [](auto settings) { return make_monte_carlo_autocallable_engine(settings).value(); });
}

TEST_CASE("Autocallable FD volatility workspaces preserve independent trials", "[fd-implied-workspace]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    const std::vector<Date> observations{day(2025, 1, 3), day(2025, 1, 8), end};
    const auto context = [&](double volatility) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar and is returned by value.
        return *make_pricing_context(*make_bsm_parameters(0.04, 0.01, volatility), 100.25,
                                     start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    };
    const auto base = context(0.27);
    const ImpliedVolatilitySettings bounds{.lower_bound = 0.1, .upper_bound = 0.4, .price_tolerance = 1e-12};
    const auto check = [&](const auto& note) {
        for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson}) {
            CAPTURE(scheme);
            const auto engine = make_finite_difference_autocallable_engine({32, 64, scheme}).value();
            std::array<double, 2> quotes{}, solutions{};
            for (std::size_t index = 0; index < quotes.size(); ++index) {
                const double volatility = index == 0 ? 0.17 : 0.33;
                const auto quote = engine.price(note, context(volatility));
                REQUIRE(quote);
                quotes[index] = *quote;
                const auto expected = detail::solve_implied_volatility(
                    engine, note, base, *quote, bounds,
                    [](const auto& scalar, const auto& instrument, const auto& shifted) {
                        return scalar.price(instrument, shifted);
                    });
                const auto actual = implied_volatility(engine, note, base, *quote, bounds);
                REQUIRE(expected);
                REQUIRE(actual);
                CHECK(*actual == *expected);
                solutions[index] = *actual;
                const auto repriced = engine.price(note, context(*actual));
                REQUIRE(repriced);
                CHECK(*repriced == Catch::Approx(*quote).margin(1e-10));
            }
            const auto rejected = implied_volatility(engine, note, base, 1e10, bounds);
            REQUIRE_FALSE(rejected);
            CHECK(rejected.error().category == ErrorCategory::unbracketed_volatility);
            auto first = std::async(std::launch::async, [&] { return implied_volatility(engine, note, base, quotes[0], bounds); });
            const auto second = implied_volatility(engine, note, base, quotes[1], bounds);
            const auto recovered = first.get();
            REQUIRE(recovered);
            REQUIRE(second);
            CHECK(*recovered == solutions[0]);
            CHECK(*second == solutions[1]);
        }
    };
    for (const auto mode : {KnockInObservationMode::every_trading_day, KnockInObservationMode::at_expiry}) {
        check(*make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}));
        check(*make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::nullopt, std::vector<double>{100.0, 99.0, 98.0})}));
        check(*make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 99.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}));
    }
    check(*make_binary_snowball_option({.maturity_coupon_rate = 0.06, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}));
}

TEST_CASE("Autocallable joint Greeks preserve scalar seeded valuations", "[autocallable-shared-samples]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    const std::vector<Date> observations{day(2025, 1, 3), day(2025, 1, 8), end};
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const NumericalShiftSettings shifts{.spot_shift = 0.2, .rate_shift = 0.25};
    const auto check = [&](const auto& note) {
        for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()})
            for (const int paths : {1, 31, 32})
                for (const double spot : {98.0, 99.0, 100.25, 101.0, 103.0})
                    for (const auto valuation : {start_of_day(start), start_of_day(start) + std::chrono::hours{12}, start_of_day(end)}) {
                        CAPTURE(seed, paths, spot, valuation.time_since_epoch().count());
                        const auto engine = make_monte_carlo_autocallable_engine({paths, seed}).value();
                        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.8), spot, valuation, calendar);
                        queries->store(0);
                        const auto expected = detail::price_with_greeks(engine, note, context, true, shifts);
                        REQUIRE(expected);
                        const int scalar_queries = queries->exchange(0);
                        const auto numerical = kiyosi::test::price_differences(engine, note, context, shifts);
                        REQUIRE(numerical);
                        if (spot == 100.25 && valuation == start_of_day(start) + std::chrono::hours{12})
                            CHECK(queries->load() < scalar_queries);
                        CHECK(numerical->price() == expected->price());
                        CHECK(numerical->values_view() == expected->values_view());
                        for (const GreeksRequest request : {GreeksRequest{Greek::rho}, GreeksRequest{Greek::delta, Greek::gamma, Greek::rho}, GreeksRequest{Greek::delta, Greek::gamma}, GreeksRequest{Greek::speed}, GreeksRequest{Greek::vanna}, GreeksRequest{Greek::zomma}, GreeksRequest{Greek::vega, Greek::vanna, Greek::zomma}}) {
                            const auto actual = engine.price_with_greeks(note, context, request, shifts);
                            REQUIRE(actual);
                            CHECK(actual->price() == expected->price());
                            CHECK(actual->values_view() == expected->selected(request).values_view());
                        }
                    }
        const auto engine = make_monte_carlo_autocallable_engine({127, 42}).value();
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.8), 100.25, start_of_day(start) + std::chrono::hours{12}, calendar);
        auto first = std::async(std::launch::async, [&] { return engine.price_with_greeks(note, context, true, shifts); });
        auto second = std::async(std::launch::async, [&] { return engine.price_with_greeks(note, context, true, shifts); });
        const auto a = first.get(), b = second.get();
        REQUIRE(a);
        REQUIRE(b);
        CHECK(a->price() == b->price());
        CHECK(a->values_view() == b->values_view());
        const auto bad_paths = make_monte_carlo_autocallable_engine({0, 42});
        REQUIRE_FALSE(bad_paths);
        CHECK(bad_paths.error().category == ErrorCategory::invalid_parameter);
        const auto bad_shifts = engine.price_with_greeks(note, context, true, {.spot_shift = 0.0});
        REQUIRE_FALSE(bad_shifts);
        CHECK(bad_shifts.error().category == ErrorCategory::invalid_parameter);
        const auto expired = *make_pricing_context(context.model_parameters(), 100.25, end + std::chrono::days{1}, calendar);
        const auto bad_time = engine.price_with_greeks(note, expired, true);
        REQUIRE_FALSE(bad_time);
        CHECK(bad_time.error().category == ErrorCategory::invalid_time_range);
        const auto unseeded = make_monte_carlo_autocallable_engine({31, std::nullopt}).value();
        REQUIRE(unseeded.price_with_greeks(note, context, true));
        CHECK_FALSE(unseeded.settings().seed);
    };
    for (const auto mode : {KnockInObservationMode::every_trading_day, KnockInObservationMode::at_expiry}) {
        check(*make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}));
        check(*make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 99.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::nullopt, std::vector<double>{100.0, 99.0, 98.0})}));
        check(*make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 99.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}));
    }
    check(*make_binary_snowball_option({.maturity_coupon_rate = 0.05, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{101.0, 100.0, 99.0}, std::vector<double>{0.08, 0.08, 0.08})}));
}

TEST_CASE("Autocallable rate batches preserve fixed prices and unavailable stencils", "[autocallable-shared-samples]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 7, 1), 101.0, 0.0}, {end, 101.0, 0.0}}});
    const auto engine = make_monte_carlo_autocallable_engine({31, 42}).value();
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 100.25,
                                               start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    const NumericalShiftSettings shifts{.rate_shift = 0.25};
    const auto expected = detail::price_with_greeks(engine, note, context, true, shifts);
    REQUIRE(expected);
    for (const GreeksRequest request : {GreeksRequest{Greek::rho}, GreeksRequest{Greek::delta, Greek::gamma, Greek::rho}}) {
        const auto actual = engine.price_with_greeks(note, context, request, shifts);
        REQUIRE(actual);
        CHECK(actual->price() == 1.0);
        CHECK(actual->values_view() == expected->selected(request).values_view());
    }
    const auto unavailable = engine.price_with_greeks(note, context, {Greek::rho}, {.rate_shift = 1e307});
    REQUIRE(unavailable);
    CHECK(unavailable->price() == 1.0);
    CHECK_FALSE(unavailable->has(Greek::rho));
    const auto rejected = engine.price_with_greeks(note, context, {Greek::rho}, {.rate_shift = 1e6});
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_result);
    const auto recovered = engine.price_with_greeks(note, context, {Greek::rho}, shifts);
    REQUIRE(recovered);
    CHECK(recovered->values_view() == expected->selected({Greek::rho}).values_view());
}

TEST_CASE("Autocallable FD event skipping preserves coupons knock-out and daily knock-in", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    // Values from the solver before non-event steps skipped the node scan.
    const std::array<std::array<double, 3>, 6> expected{{
        {0.9851073493143833, 0.0033442626878479875, -0.0003388868715960558},
        {0.985121983280094, 0.0033434330011221924, -0.0003390186073992385},
        {0.985092706060584, 0.003345091076280171, -0.00033875459128074326},
        {0.9741963514308023, 0.004327134341220007, -0.00039946361635084804},
        {0.9742415857695996, 0.004324391125183735, -0.00039959520738663823},
        {0.9741507369014278, 0.004329894964145986, -0.00039933080007825694},
    }};
    std::size_t index = 0;
    for (const auto mode : {KnockInObservationMode::at_expiry, KnockInObservationMode::every_trading_day}) {
        const auto note = *make_phoenix_option(
            {.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 4, 1), 110.0, 0.0, 90.0}, {day(2025, 7, 1), 108.0, 0.0, 90.0}, {day(2025, 10, 1), 106.0, 0.0, 90.0}, {end, 104.0, 0.0, 90.0}}});
        for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::explicit_euler}) {
            CAPTURE(mode, scheme);
            const auto result = make_finite_difference_autocallable_engine({200, 2000, scheme, 400.0}).value().price_with_greeks(note, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK(result->price() == Catch::Approx(expected[index][0]).margin(1e-12));
            CHECK(test::greek_value(*result, Greek::delta) == Catch::Approx(expected[index][1]).margin(1e-12));
            CHECK(test::greek_value(*result, Greek::gamma) == Catch::Approx(expected[index][2]).margin(1e-12));
            ++index;
        }
    }
}

TEST_CASE("Expiry-only knock-in layers reproduce analytic terminal portfolios", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    const auto put = [&](double strike) {
        return *AnalyticVanillaEngine{}.price(*make_european_option(OptionType::put, strike, start, end), context);
    };
    const double discount = std::exp(-0.04);
    const double downside = discount - (put(100.0) - put(60.0)) / 100.0;
    const double digital = *AnalyticDigitalEngine{}.price(*make_cash_or_nothing_option(OptionType::call, 100.0, 1.0, start, end), context);
    const auto snowball = *make_snowball_option(
        {.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 100.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.06}}});
    const auto phoenix = *make_phoenix_option(
        {.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 100.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.0, 0.0}}});
    const auto ternary = *make_ternary_snowball_option(
        {.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 100.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.06}}});
    for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::explicit_euler}) {
        const auto check = [&](const auto& note, double expected) {
            const auto engine = make_finite_difference_autocallable_engine({200, 2000, scheme, 400.0}).value();
            const auto actual = engine.price_with_greeks(note, context, {Greek::delta, Greek::gamma});
            REQUIRE(actual);
            CHECK(actual->price() == Catch::Approx(expected).margin(0.002));
            CHECK(actual->has(Greek::delta));
            CHECK(actual->has(Greek::gamma));
        };
        CAPTURE(scheme);
        check(snowball, downside + 0.06 * digital);
        check(phoenix, downside + 0.08 * discount);
        check(ternary, 1.02 * discount + 0.04 * digital);
    }
}

TEST_CASE("Autocallables omit daily monitoring only when knock-in cannot change settlement", "[monte-carlo-performance][fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, calendar);
    const auto check = [&](const auto& daily, const auto& expiry, bool affects_payoff) {
        const auto check_engine = [&](const auto& engine) {
            queries->store(0);
            const auto actual = engine.price(daily, context);
            REQUIRE(actual);
            CHECK((queries->load() > 300) == affects_payoff);
            const auto expected = engine.price(expiry, context);
            REQUIRE(expected);
            if (!affects_payoff) CHECK(*actual == *expected);
        };
        check_engine(make_monte_carlo_autocallable_engine({2'000, 42}).value());
        for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler})
            check_engine(make_finite_difference_autocallable_engine({200, 200, scheme}).value());
    };
    for (const double lower : {60.0, 100.0})
        for (const double coupon : {0.0, 0.06}) {
            SnowballTerms terms{.maturity_coupon_rate = coupon, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = lower, .effective_date = start, .expiry_date = end, .observations = {{middle, 110.0, 0.08}, {end, 104.0, 0.08}}};
            const auto daily = *make_snowball_option(terms);
            terms.knock_in_observation_mode = KnockInObservationMode::at_expiry;
            check(daily, *make_snowball_option(terms), lower != 100.0 || coupon != 0.0);
        }
    for (const double minimum : {0.02, 0.06}) {
        TernarySnowballTerms terms{.maturity_coupon_rate = 0.06, .minimum_coupon_rate = minimum, .knock_in_level = 75.0, .effective_date = start, .expiry_date = end, .observations = {{middle, 110.0, 0.08}, {end, 104.0, 0.08}}};
        const auto daily = *make_ternary_snowball_option(terms);
        terms.knock_in_observation_mode = KnockInObservationMode::at_expiry;
        check(daily, *make_ternary_snowball_option(terms), minimum != 0.06);
    }
    PhoenixTerms terms{.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 100.0, .effective_date = start, .expiry_date = end, .observations = {{middle, 110.0, 0.0, 90.0}, {end, 104.0, 0.0, 90.0}}};
    const auto daily = *make_phoenix_option(terms);
    terms.knock_in_observation_mode = KnockInObservationMode::at_expiry;
    check(daily, *make_phoenix_option(terms), false);
}

TEST_CASE("Protected FD notes retain daily explicit stability subdivisions", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, all_days_calendar());
    SnowballTerms terms{.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 100.0, .effective_date = start, .expiry_date = end, .observations = {{middle, 110.0, 0.08}, {end, 104.0, 0.08}}};
    const auto daily = *make_snowball_option(terms);
    terms.knock_in_observation_mode = KnockInObservationMode::at_expiry;
    const auto expiry = *make_snowball_option(terms);
    const auto engine = make_finite_difference_autocallable_engine({80, 200, FiniteDifferenceScheme::explicit_euler, 400.0}).value();
    const auto actual = engine.price(daily, context);
    REQUIRE(actual);
    const auto reference = make_finite_difference_autocallable_engine({80, 2000, FiniteDifferenceScheme::explicit_euler, 400.0}).value().price(expiry, context);
    REQUIRE(reference);
    CHECK(*actual == Catch::Approx(*reference).margin(0.0002));
    const auto unstable = engine.price(expiry, context);
    REQUIRE_FALSE(unstable);
    CHECK(unstable.error().category == ErrorCategory::invalid_parameter);
}

TEST_CASE("Knocked-in finite-difference autocallables preserve coupons and downside", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), valuation = day(2025, 1, 2), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 70.0, valuation, calendar);
    const auto put_price = [&](double strike) {
        return *AnalyticVanillaEngine{}.price(*make_european_option(OptionType::put, strike, start, end), context);
    };
    const auto discount = [&](Date date) { return std::exp(-0.04 * *year_fraction(valuation, date)); };
    const double downside = discount(end) - (put_price(100.0) - put_price(60.0)) / 100.0;
    const double coupons = 0.08 * (*year_fraction(start, valuation) +
                                   *year_fraction(valuation, middle) * discount(middle) +
                                   *year_fraction(middle, end) * discount(end));
    for (const auto state : {AutocallableBarrierState::none, AutocallableBarrierState::knocked_in}) {
        const auto snowball = *make_snowball_option(
            {.maturity_coupon_rate = 0.8, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = state, .effective_date = start, .expiry_date = end, .observations = {{valuation, 200.0, 0.1}, {middle, 200.0, 0.1}, {end, 200.0, 0.1}}});
        const auto phoenix = *make_phoenix_option(
            {.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = state, .effective_date = start, .expiry_date = end, .observations = {{valuation, 200.0, 0.0, 0.0}, {middle, 200.0, 0.0, 0.0}, {end, 200.0, 0.0, 0.0}}});
        const auto ternary = *make_ternary_snowball_option(
            {.maturity_coupon_rate = 0.8, .minimum_coupon_rate = 0.02, .knock_in_level = 75.0, .barrier_state = state, .effective_date = start, .expiry_date = end, .observations = {{valuation, 200.0, 0.1}, {middle, 200.0, 0.1}, {end, 200.0, 0.1}}});
        for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler}) {
            const FiniteDifferenceSettings settings{.asset_step_count = 800, .time_step_count = 400, .scheme = scheme, .asset_upper_boundary = 800.0};
            const auto check = [&](const auto& note, double expected) {
                const auto engine = make_finite_difference_autocallable_engine(settings).value();
                queries->store(0);
                const auto price = engine.price(note, context);
                CHECK(queries->load() < 10);
                const auto result = engine.price_with_greeks(note, context, {Greek::delta, Greek::gamma});
                REQUIRE(price);
                REQUIRE(result);
                CHECK(*price == Catch::Approx(expected).margin(0.0002));
                CHECK(result->price() == *price);
                CHECK(result->has(Greek::delta));
                CHECK(result->has(Greek::gamma));
            };
            CAPTURE(state, scheme);
            check(snowball, downside);
            check(phoenix, downside + coupons);
            check(ternary, 1.02 * discount(end));
        }
        const auto explicit_price = make_finite_difference_autocallable_engine({80, 200, FiniteDifferenceScheme::explicit_euler, 800.0}).value().price(ternary, context);
        REQUIRE(explicit_price);
        CHECK(*explicit_price == Catch::Approx(1.02 * discount(end)).margin(0.0002));
    }
}

TEST_CASE("Autocallable finite-difference spot Greeks reuse one solve", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 70.0,
                                               day(2025, 1, 2), calendar);
    const auto note = *make_snowball_option(
        {.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 0.0, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end, .observations = {{end, 200.0, 0.0}}});
    const auto engine = make_finite_difference_autocallable_engine({.asset_step_count = 800, .time_step_count = 200, .asset_upper_boundary = 800.0}).value();
    const auto price = engine.price(note, context);
    REQUIRE(price);
    const int price_queries = queries->exchange(0);
    const auto result = engine.price_with_greeks(note, context, {Greek::delta, Greek::gamma, Greek::speed});
    REQUIRE(result);
    CHECK(queries->load() == price_queries);
    CHECK(result->price() == *price);
    CHECK(result->has(Greek::speed));
    const auto put = *make_european_option(OptionType::put, 100.0, start, end);
    const auto expected = AnalyticVanillaEngine{}.price_with_greeks(put, context, {Greek::delta, Greek::gamma});
    REQUIRE(expected);
    CHECK(test::greek_value(*result, Greek::delta) == Catch::Approx(-test::greek_value(*expected, Greek::delta) / 100.0).margin(1e-5));
    CHECK(test::greek_value(*result, Greek::gamma) == Catch::Approx(-test::greek_value(*expected, Greek::gamma) / 100.0).margin(1e-6));
    CHECK_FALSE(result->has(Greek::vega));
}

TEST_CASE("Knocked-in autocallables simulate only remaining observation dates", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), valuation = day(2025, 1, 2), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 70.0, valuation, calendar);
    const auto note = *make_snowball_option(
        {.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 0.0, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end, .observations = {{end, 1e6, 0.0}}});
    const auto actual = make_monte_carlo_autocallable_engine({50'000, 42}).value().price(note, context);
    REQUIRE(actual);
    // Calendar validation remains necessary; daily continuation queries do not.
    CHECK(queries->load() < 10);
    const auto put = *make_european_option(OptionType::put, 100.0, start, end);
    const auto expected = AnalyticVanillaEngine{}.price(put, context);
    REQUIRE(expected);
    CHECK(*actual == Catch::Approx(1.0 - *expected / 100.0).margin(0.002));
}

TEST_CASE("Fixed autocallable implied volatility rejects before simulation", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 100.0, start, calendar);
    const auto note = *make_snowball_option(
        {.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 100.0, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.0}}});
    const auto engine = make_monte_carlo_autocallable_engine({20'000, 42}).value();
    const auto implied = implied_volatility(engine, note, context, 1.0);
    REQUIRE_FALSE(implied);
    CHECK(implied.error().category == ErrorCategory::unsupported_operation);
    CHECK(queries->exchange(0) < 20);
    const auto cuda = implied_volatility(make_monte_carlo_autocallable_engine({20'000, 42, MonteCarloBackend::cuda}).value(), note, context, 1.0);
    REQUIRE_FALSE(cuda);
    CHECK(cuda.error().category == ErrorCategory::unsupported_operation);
    const auto invalid = make_monte_carlo_autocallable_engine({0, 42});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    const auto later = *make_pricing_context(context.model_parameters(), 100.0, start + std::chrono::days{1}, calendar);
    const auto missing_history = implied_volatility(engine, note, later, 1.0);
    REQUIRE_FALSE(missing_history);
    CHECK(missing_history.error().category == ErrorCategory::invalid_parameter);
    const auto closed = *make_trading_calendar([](Date) { return false; }, 365);
    const auto closed_context = *make_pricing_context(context.model_parameters(), 100.0, start, closed);
    const auto invalid_calendar = implied_volatility(engine, note, closed_context, 1.0);
    REQUIRE_FALSE(invalid_calendar);
    CHECK(invalid_calendar.error().category == ErrorCategory::invalid_date);
}

TEST_CASE("Fixed autocallable prices skip numerical work after validation", "[monte-carlo-performance][fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 100.0, start, calendar);
    const auto note = *make_snowball_option(
        {.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 100.0, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.0}}});
    for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda}) {
        queries->store(0);
        const auto price = make_monte_carlo_autocallable_engine({20'000, 42, backend}).value().price(note, context);
        REQUIRE(price);
        CHECK(*price == 1.0);
        CHECK(queries->load() < 10);
    }
    // A fixed payoff needs no spatial domain, even when a configured grid would clip it.
    const auto finite_difference = make_finite_difference_autocallable_engine({.asset_upper_boundary = 1.0}).value();
    queries->store(0);
    const auto fixed = finite_difference.price_with_greeks(note, context, {Greek::delta, Greek::gamma});
    REQUIRE(fixed);
    CHECK(fixed->price() == 1.0);
    CHECK(test::greek_value(*fixed, Greek::delta) == 0.0);
    CHECK(test::greek_value(*fixed, Greek::gamma) == 0.0);
    CHECK(queries->load() < 10);
    const auto invalid_grid = make_finite_difference_autocallable_engine({.time_step_count = 0});
    REQUIRE_FALSE(invalid_grid);
    CHECK(invalid_grid.error().category == ErrorCategory::invalid_parameter);
    const auto invalid = make_monte_carlo_autocallable_engine({0, 42});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    const auto later = *make_pricing_context(context.model_parameters(), 100.0, start + std::chrono::days{1}, calendar);
    const auto missing_history = make_monte_carlo_autocallable_engine({20'000, 42}).value().price(note, later);
    REQUIRE_FALSE(missing_history);
    CHECK(missing_history.error().category == ErrorCategory::invalid_parameter);
    const auto fd_missing_history = finite_difference.price(note, later);
    REQUIRE_FALSE(fd_missing_history);
    CHECK(fd_missing_history.error().category == ErrorCategory::invalid_parameter);
}

TEST_CASE("Fixed finite-difference autocallables retain rate sensitivity", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    const auto note = *make_binary_snowball_option(
        {.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.0}}});
    const auto fixed = FiniteDifferenceAutocallableEngine{}.price_with_greeks(
        note, context, {Greek::delta, Greek::gamma, Greek::vega, Greek::rho});
    REQUIRE(fixed);
    CHECK(fixed->price() == std::exp(-0.04));
    CHECK(test::greek_value(*fixed, Greek::delta) == 0.0);
    CHECK(test::greek_value(*fixed, Greek::gamma) == 0.0);
    CHECK(test::greek_value(*fixed, Greek::vega) == 0.0);
    CHECK(test::greek_value(*fixed, Greek::rho) == Catch::Approx(-std::exp(-0.04) / 100.0).margin(1e-8));
}

TEST_CASE("Fixed autocallable implied volatility preserves non-finite pricing errors", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto note = make_binary_snowball_option(
        {.maturity_coupon_rate = 1e308, .principal_ratio = 1e308, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 1e308}}});
    REQUIRE(note);
    for (const Date date : {start, end}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 100.0, date);
        const auto result = implied_volatility(make_monte_carlo_autocallable_engine({2, 1}).value(), *note, context, 1.0);
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::invalid_result);
    }
}

TEST_CASE("Seeded autocallable paths stay coupled when knock-out dates change")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto note = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.2, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 7, 1), 110.0, 0.0}, {end, 110.0, 0.0}}});
    const auto parameters = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2);
    for (const std::uint64_t seed : {std::uint64_t{1}, std::numeric_limits<std::uint64_t>::max()}) {
        CAPTURE(seed);
        const auto engine = kiyosi::make_monte_carlo_autocallable_engine({2000, seed}).value();
        double previous = 1.2;
        for (int bump = 0; bump <= 30; ++bump) {
            const double spot = 100.0 + 0.01 * bump;
            CAPTURE(spot);
            const auto context = *kiyosi::make_pricing_context(parameters, spot, start);
            const auto actual = engine.price(note, context);
            REQUIRE(actual);
            // On the same paths, earlier zero-coupon knock-out can only reduce this payout.
            CHECK(*actual <= previous);
            previous = *actual;
        }
    }
}

TEST_CASE("Autocallable engines validate observation dates against each market calendar")
{
    const auto effective = day(2025, 1, 3);
    const auto expiry = day(2025, 1, 6);
    const auto note = *kiyosi::make_binary_snowball_option(
        {.maturity_coupon_rate = 0.1, .effective_date = effective, .expiry_date = expiry, .observations = {{day(2025, 1, 4), 120.0, 0.1}}});
    for (const auto& calendar : {kiyosi::all_days_calendar(), kiyosi::weekdays_calendar()}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.01, 0.0, 0.2), 100.0, effective, calendar);
        for (const auto& result : {
                 kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value().price(note, context),
                 kiyosi::make_finite_difference_autocallable_engine({40, 40}).value().price(note, context)}) {
            if (calendar.is_trading_day(day(2025, 1, 4))) {
                REQUIRE(result);
                CHECK(std::isfinite(*result));
            } else {
                REQUIRE_FALSE(result);
                CHECK(result.error().category == kiyosi::ErrorCategory::invalid_date);
                CHECK(result.error().message == "observation Date is not a trading day");
            }
        }
    }
}

TEST_CASE("Autocallable engines reject nominal weekend expiry and accept adjusted expiry")
{
    const auto nominal = day(2025, 1, 5);
    const auto calendar = kiyosi::weekdays_calendar();
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.01, 0.0, 0.2), 100.0, day(2025, 1, 3), calendar);
    const auto note = [&](kiyosi::Date expiry) {
        return *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.01, .effective_date = day(2025, 1, 3), .expiry_date = expiry, .observations = {{day(2025, 1, 3), 200.0, 0.0}}});
    };
    const auto unadjusted = note(nominal);
    const auto adjusted_expiry = calendar.adjust(nominal, kiyosi::BusinessDayConvention::following);
    REQUIRE(adjusted_expiry);
    const auto adjusted = note(*adjusted_expiry);
    const auto mc = kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value();
    const kiyosi::FiniteDifferenceAutocallableEngine fd{};
    for (const auto& result : {mc.price(unadjusted, context), fd.price(unadjusted, context)}) {
        REQUIRE_FALSE(result);
        CHECK(result.error().category == kiyosi::ErrorCategory::invalid_date);
    }
    CHECK(mc.price(adjusted, context).has_value());
    CHECK(fd.price(adjusted, context).has_value());
}

double reference_binary_snowball_price(const kiyosi::BinarySnowballOption& note,
                                       const kiyosi::PricingContext& context,
                                       kiyosi::TradingDayMonteCarloSettings settings)
{
    const auto path_payoff = [&](kiyosi::detail::Pcg32& generator) {
        if (note.barrier_state() == kiyosi::AutocallableBarrierState::knocked_out) return 0.0;

        const double rate = context.model_parameters().risk_free_rate();
        const double dividend = context.model_parameters().dividend_yield();
        const double sigma = context.model_parameters().volatility();
        const auto valuation = context.valuation_time();
        const auto observations = note.observations_view();
        std::vector<std::size_t> schedule;
        for (std::size_t index = 0; index < observations.size(); ++index)
            if (observations[index].date >= valuation) schedule.push_back(index);

        double value = context.spot_price();
        std::size_t index = 0;
        if (!schedule.empty() && observations[schedule.front()].date == valuation) {
            const auto event = schedule.front();
            const double coupon = observations[event].coupon_rate *
                                  *kiyosi::year_fraction(note.effective_date(), observations[event].date);
            if (value >= observations[event].knock_out_level) return note.principal_ratio() + coupon;
            index = 1;
        }
        const double terminal = note.principal_ratio() +
                                note.maturity_coupon_rate() *
                                    *kiyosi::year_fraction(note.effective_date(), note.expiry_date());
        if (valuation == note.expiry_date()) return terminal;

        std::normal_distribution<double> normal;
        auto previous = valuation;
        for (; index < schedule.size(); ++index) {
            const auto current = observations[schedule[index]].date;
            const double dt = *kiyosi::year_fraction(previous, kiyosi::start_of_day(current));
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt +
                              sigma * std::sqrt(dt) * normal(generator));
            previous = kiyosi::start_of_day(current);
            const auto event = schedule[index];
            const double coupon = observations[event].coupon_rate *
                                  *kiyosi::year_fraction(note.effective_date(), observations[event].date);
            if (value >= observations[event].knock_out_level)
                return (note.principal_ratio() + coupon) *
                       std::exp(-rate * *kiyosi::year_fraction(valuation, previous));
        }
        return std::exp(-rate *
                        *kiyosi::year_fraction(valuation, kiyosi::start_of_day(note.expiry_date()))) *
               terminal;
    };

    double sum = 0.0;
    for (int path = 0; path < settings.path_count; ++path) {
        kiyosi::detail::Pcg32 path_generator{settings.seed.value_or(0), static_cast<std::uint64_t>(path)};
        sum += path_payoff(path_generator);
    }
    return sum / static_cast<double>(settings.path_count);
}

TEST_CASE("Phoenix expiry_date settlement applies state and final observations")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry_date);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value().price(note, market(spot));
        REQUIRE(result);
        return *result;
    };

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 100.0, 0.0, 90.0}}});
    CHECK(price(phoenix, 100.0) == Catch::Approx(1.08));
    CHECK(price(phoenix, 80.0) == Catch::Approx(1.0));
    const auto phoenix_up = *kiyosi::make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::knocked_out, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{day(2025, 7, 1), 100.0, 0.0, 90.0}, {expiry_date, 100.0, 0.0, 90.0}}});
    CHECK(price(phoenix_up, 100.0) == 0.0);
}

TEST_CASE("Snowball expiry_date settlement applies state and final observations")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry_date);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value().price(note, market(spot));
        REQUIRE(result);
        return *result;
    };
    const auto snowball = *kiyosi::make_snowball_option({.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 100.0, 0.10}}});
    CHECK(price(snowball, 100.0) == Catch::Approx(1.10));
    CHECK(price(snowball, 80.0) == Catch::Approx(1.05));
    CHECK(price(snowball, 59.0) == Catch::Approx(0.60));
    const auto snowball_down = *kiyosi::make_snowball_option({.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::knocked_in, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, 0.10}}});
    CHECK(price(snowball_down, 70.0) == Catch::Approx(0.70));
}

TEST_CASE("Autocallable Monte Carlo skips knock-in on a non-trading valuation day")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2025, 1, 6);
    const auto note = *kiyosi::make_snowball_option(
        {.maturity_coupon_rate = 0.12, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .effective_date = effective, .expiry_date = expiry, .observations = {{expiry, 120.0, 0.12}}});
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto saturday = kiyosi::start_of_day(day(2025, 1, 4));
    const auto midnight = *kiyosi::make_pricing_context(parameters, 79.0, saturday);
    const auto later = *kiyosi::make_pricing_context(parameters, 79.0, saturday + std::chrono::microseconds{1});
    const auto engine = kiyosi::make_monte_carlo_autocallable_engine({20'000, 1}).value();
    const auto first = engine.price(note, midnight);
    const auto second = engine.price(note, later);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(std::abs(*first - *second) < 1e-7);
}

TEST_CASE("Binary snowball expiry_date settlement applies final observations")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry_date);
    };
    const auto price = [&](const auto& engine, const auto& note, double spot) {
        const auto result = engine.price(note, market(spot));
        REQUIRE(result);
        return *result;
    };
    const auto binary = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.05, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 100.0, 0.10}}});
    const auto check = [&](const auto& engine) {
        CHECK(price(engine, binary, 100.0) == Catch::Approx(1.10));
        CHECK(price(engine, binary, 90.0) == Catch::Approx(1.05));
    };
    check(kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value());
    check(kiyosi::FiniteDifferenceAutocallableEngine{});
}

TEST_CASE("Ternary snowball expiry_date settlement applies final observations")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry_date);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value().price(note, market(spot));
        REQUIRE(result);
        return *result;
    };
    const auto ternary = *kiyosi::make_ternary_snowball_option({.maturity_coupon_rate = 0.05, .minimum_coupon_rate = 0.02, .knock_in_level = 80.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 100.0, 0.10}}});
    CHECK(price(ternary, 100.0) == Catch::Approx(1.10));
    CHECK(price(ternary, 80.0) == Catch::Approx(1.05));
    CHECK(price(ternary, 79.0) == Catch::Approx(1.02));
}

TEST_CASE("Autocallable Monte Carlo processes valuation-date observation events once")
{
    const auto effective_date = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12), 100.0, valuation);
    const auto monte_carlo_price = [&](const auto& instrument) {
        const auto result = kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value().price(instrument, context);
        REQUIRE(result);
        return *result;
    };
    const auto note = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.05, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{effective_date, 90.0, 99.0}, {valuation, 90.0, 10.0}, {expiry_date, 99.0, 0.10}}});
    const auto engine = kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value();
    const auto first = engine.price(note, context);
    const auto second = engine.price(note, context);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(*first == *second);
    CHECK(*first ==
          Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective_date, valuation).value()).epsilon(0.0).margin(1e-10));
    for (const auto elapsed : {std::chrono::hours{12}, std::chrono::hours{24}}) {
        const auto later_context = *kiyosi::make_pricing_context(
            context.model_parameters(), 100.0, kiyosi::start_of_day(valuation) + elapsed);
        const auto later = engine.price(note, later_context);
        REQUIRE(later);
        CHECK(*later == Catch::Approx(
                            1.0 + 0.10 * kiyosi::year_fraction(effective_date, expiry_date).value())
                            .epsilon(0.0)
                            .margin(1e-10));
    }
    const auto snowball = *kiyosi::make_snowball_option({.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{effective_date, 90.0, 99.0}, {valuation, 90.0, 10.0}, {expiry_date, 99.0, 0.10}}});
    CHECK(monte_carlo_price(snowball) == Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective_date, valuation).value()));

    const auto ternary = *kiyosi::make_ternary_snowball_option({.maturity_coupon_rate = 0.05, .minimum_coupon_rate = 0.02, .knock_in_level = 80.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{effective_date, 90.0, 99.0}, {valuation, 90.0, 10.0}, {expiry_date, 99.0, 0.10}}});
    CHECK(monte_carlo_price(ternary) == Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective_date, valuation).value()));

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{effective_date, 90.0, 0.0, 90.0}, {valuation, 90.0, 0.0, 90.0}, {expiry_date, 99.0, 0.0, 90.0}}});
    CHECK(monte_carlo_price(phoenix) ==
          Catch::Approx(1.0 + 0.08 * kiyosi::year_fraction(effective_date, valuation).value()));
}

TEST_CASE("Autocallable Monte Carlo settles deterministic states before simulation")
{
    const auto effective_date = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date) {
            ++*calls;
            return true;
        },
        365);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), 100.0, valuation, calendar);
    const auto make_note = [&](kiyosi::AutocallableBarrierState barrier_state, double principal,
                               double coupon = 0.0) {
        return *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.0, .barrier_state = barrier_state, .principal_ratio = principal, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{valuation, 100.0, coupon}, {expiry_date, 100.0, 0.0}}});
    };

    const double large_payoff = std::numeric_limits<double>::max() / 64.0;
    const auto immediate = kiyosi::make_monte_carlo_autocallable_engine({128, 7}).value().price(
        make_note(kiyosi::AutocallableBarrierState::none, large_payoff), context);
    REQUIRE(immediate);
    CHECK(*immediate == large_payoff);
    CHECK(calls->load() == 3);

    calls->store(0);
    const auto touched = kiyosi::make_monte_carlo_autocallable_engine({128, 7}).value().price(
        make_note(kiyosi::AutocallableBarrierState::knocked_out, 1.0), context);
    REQUIRE_FALSE(touched);
    CHECK(touched.error().category == kiyosi::ErrorCategory::invalid_option);
    CHECK(calls->load() == 2);

    calls->store(0);
    const auto expiry_context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), 90.0, expiry_date, calendar);
    const auto expiry_note = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.0, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = large_payoff, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 100.0, 0.0}}});
    const auto at_expiry = kiyosi::make_monte_carlo_autocallable_engine({128, 7}).value().price(
        expiry_note, expiry_context);
    REQUIRE(at_expiry);
    CHECK(*at_expiry == large_payoff);
    CHECK(calls->load() == 2);

    calls->store(0);
    const auto invalid_settings = kiyosi::make_monte_carlo_autocallable_engine({0, 7});
    REQUIRE_FALSE(invalid_settings);
    CHECK(invalid_settings.error().category == kiyosi::ErrorCategory::invalid_parameter);
    CHECK(calls->load() == 0);

    const auto non_finite = kiyosi::make_monte_carlo_autocallable_engine({128, 7}).value().price(
        make_note(kiyosi::AutocallableBarrierState::none, std::numeric_limits<double>::max(),
                  std::numeric_limits<double>::max()),
        context);
    REQUIRE_FALSE(non_finite);
    CHECK(non_finite.error().category == kiyosi::ErrorCategory::invalid_result);
}

TEST_CASE("Autocallable Monte Carlo prepares stable calendar inputs once")
{
    const auto valuation = day(2025, 1, 1);
    const auto first_observation = day(2025, 1, 4);
    const auto expiry_date = day(2025, 1, 6);
    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date value) {
            ++*calls;
            return value == valuation || value == day(2025, 1, 2) ||
                   value == first_observation || value == expiry_date;
        },
        252);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.0, 0.2), 100.0, valuation, calendar);
    const auto note = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.02, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = valuation, .expiry_date = expiry_date, .observations = {{first_observation, 100.0, 0.10}, {expiry_date, 100.0, 0.20}}});
    const std::array seeds{std::uint64_t{0}, std::numeric_limits<std::uint64_t>::max()};
    for (const std::uint64_t seed : seeds) {
        CAPTURE(seed);
        const kiyosi::TradingDayMonteCarloSettings settings{32, seed};
        const double expected = reference_binary_snowball_price(note, context, settings);
        calls->store(0);

        const auto result = kiyosi::make_monte_carlo_autocallable_engine(settings).value().price(note, context);

        REQUIRE(result);
        CHECK(*result == Catch::Approx(expected).epsilon(0.0).margin(1e-12));
        CHECK(calls->load() == 3);
    }

    const auto unseeded =
        kiyosi::make_monte_carlo_autocallable_engine({32, std::nullopt}).value().price(note, context);
    REQUIRE(unseeded);
    CHECK(std::isfinite(*unseeded));
}

TEST_CASE("Autocallable CUDA selection validates and preserves deterministic settlements")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2025, 1, 6);
    const auto make_note = [&](kiyosi::AutocallableBarrierState barrier_state) {
        return *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.05, .barrier_state = barrier_state, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, 0.08}}});
    };
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.01, 0.2), 100.0, effective_date,
        kiyosi::all_days_calendar());
    const auto cuda = kiyosi::make_monte_carlo_autocallable_engine({64, 7, kiyosi::MonteCarloBackend::cuda}).value();
    CHECK(cuda.settings().backend == kiyosi::MonteCarloBackend::cuda);

    const auto settled_note = *kiyosi::make_binary_snowball_option(
        {.maturity_coupon_rate = 0.05, .barrier_state = kiyosi::AutocallableBarrierState::knocked_out, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{day(2025, 1, 2), 110.0, 0.08}, {expiry_date, 110.0, 0.08}}});
    const auto settled_context = *kiyosi::make_pricing_context(
        context.model_parameters(), 100.0, day(2025, 1, 3), kiyosi::all_days_calendar());
    const auto settled = cuda.price(settled_note, settled_context);
    REQUIRE(settled);
    CHECK(*settled == 0.0);

    const auto invalid = kiyosi::make_monte_carlo_autocallable_engine({0, 7, kiyosi::MonteCarloBackend::cuda});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto invalid_backend = kiyosi::make_monte_carlo_autocallable_engine({64, 7, static_cast<kiyosi::MonteCarloBackend>(255)});
    REQUIRE_FALSE(invalid_backend);
    CHECK(invalid_backend.error().category == kiyosi::ErrorCategory::invalid_parameter);

#if !KIYOSI_HAS_CUDA
    const auto unavailable =
        cuda.price(make_note(kiyosi::AutocallableBarrierState::none), context);
    REQUIRE_FALSE(unavailable);
    CHECK(unavailable.error().category == kiyosi::ErrorCategory::backend_unavailable);
#endif
}

#if KIYOSI_HAS_CUDA
TEST_CASE("Autocallable CUDA Monte Carlo prices every public autocallable engine")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2025, 4, 1);
    const std::vector<kiyosi::Date> observations{
        day(2025, 2, 1), day(2025, 3, 1), expiry_date};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.01, 0.2), 100.0, effective_date,
        kiyosi::all_days_calendar());
    const kiyosi::TradingDayMonteCarloSettings cpu_settings{
        32'768, 73, kiyosi::MonteCarloBackend::cpu};
    const kiyosi::TradingDayMonteCarloSettings cuda_settings{
        32'768, 73, kiyosi::MonteCarloBackend::cuda};
    const auto check = [&](const auto& note, const auto& cpu_engine, const auto& cuda_engine) {
        const auto cpu = cpu_engine.price(note, context);
        auto first_future = std::async(
            std::launch::async, [&] { return cuda_engine.price(note, context); });
        auto second_future = std::async(
            std::launch::async, [&] { return cuda_engine.price(note, context); });
        const auto first = first_future.get();
        const auto second = second_future.get();
        REQUIRE(cpu);
        REQUIRE(first);
        REQUIRE(second);
        const double cuda_price = *first;
        CHECK(cuda_price == *second);
        CHECK(cuda_price == Catch::Approx(*cpu)
                                .margin(0.04));
    };

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.002, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 108.0, 105.0}, std::nullopt, std::vector<double>{95.0, 95.0, 95.0})});
    check(phoenix, kiyosi::make_monte_carlo_autocallable_engine(cpu_settings).value(),
          kiyosi::make_monte_carlo_autocallable_engine(cuda_settings).value());

    const auto snowball = *kiyosi::make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 108.0, 105.0}, std::vector<double>{0.08, 0.08, 0.08})});
    check(snowball, kiyosi::make_monte_carlo_autocallable_engine(cpu_settings).value(),
          kiyosi::make_monte_carlo_autocallable_engine(cuda_settings).value());

    const auto binary = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.06, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 108.0, 105.0}, std::vector<double>{0.08, 0.08, 0.08})});
    check(binary, kiyosi::make_monte_carlo_autocallable_engine(cpu_settings).value(),
          kiyosi::make_monte_carlo_autocallable_engine(cuda_settings).value());

    const auto ternary = *kiyosi::make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.01, .knock_in_level = 80.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 108.0, 105.0}, std::vector<double>{0.08, 0.08, 0.08})});
    check(ternary, kiyosi::make_monte_carlo_autocallable_engine(cpu_settings).value(),
          kiyosi::make_monte_carlo_autocallable_engine(cuda_settings).value());

    const auto different_seed = kiyosi::make_monte_carlo_autocallable_engine({32'768, 74, kiyosi::MonteCarloBackend::cuda}).value().price(phoenix, context);
    const auto original_seed =
        kiyosi::make_monte_carlo_autocallable_engine(cuda_settings).value().price(phoenix, context);
    REQUIRE(different_seed);
    REQUIRE(original_seed);
    CHECK(*different_seed !=
          *original_seed);
}

TEST_CASE("Autocallable CUDA Monte Carlo preserves coupons and historical touch state")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2025, 1, 4);
    const auto context = [&](double spot, kiyosi::Date valuation) {
        return *kiyosi::make_pricing_context( // NOLINT(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar; MSVC std::function copies are misdiagnosed.
            *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-8), spot, valuation,
            kiyosi::all_days_calendar());
    };
    const kiyosi::TradingDayMonteCarloSettings settings{
        4'096, 73, kiyosi::MonteCarloBackend::cuda};

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.01, .initial_spot = 100.0, .knock_in_level = 50.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{day(2025, 1, 2), 200.0, 0.0, 90.0}, {day(2025, 1, 3), 200.0, 0.0, 90.0}, {expiry_date, 200.0, 0.0, 90.0}}});
    const auto phoenix_result =
        kiyosi::make_monte_carlo_autocallable_engine(settings).value().price(phoenix, context(100.0, effective_date));
    REQUIRE(phoenix_result);
    CHECK(*phoenix_result ==
          Catch::Approx(1.0 + 0.01 * 3.0 / 365.0).margin(1e-10).epsilon(0.0));

    const auto snowball = *kiyosi::make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::knocked_in, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 200.0, 0.08}}});
    const auto snowball_result =
        kiyosi::make_monte_carlo_autocallable_engine(settings).value().price(snowball, context(70.0, day(2025, 1, 2)));
    REQUIRE(snowball_result);
    CHECK(*snowball_result ==
          Catch::Approx(0.7).epsilon(0.0).margin(1e-8));

    const auto ternary = *kiyosi::make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.01, .knock_in_level = 80.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::knocked_in, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 200.0, 0.08}}});
    const auto ternary_result =
        kiyosi::make_monte_carlo_autocallable_engine(settings).value().price(ternary, context(100.0, day(2025, 1, 2)));
    REQUIRE(ternary_result);
    CHECK(*ternary_result ==
          Catch::Approx(1.0 + 0.01 * 3.0 / 365.0).epsilon(0.0).margin(1e-12));

    const auto binary = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.06, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 200.0, 0.08}}});
    const auto binary_result =
        kiyosi::make_monte_carlo_autocallable_engine(settings).value().price(binary, context(100.0, effective_date));
    REQUIRE(binary_result);
    CHECK(*binary_result ==
          Catch::Approx(1.0 + 0.06 * 3.0 / 365.0).epsilon(0.0).margin(1e-12));
}
#endif

TEST_CASE("Autocallable finite difference preserves future observation indices")
{
    const auto effective_date = day(2025, 1, 1);
    const auto valuation_date = day(2025, 1, 2);
    const auto future_observation = day(2025, 1, 4);
    const auto expiry_date = day(2025, 1, 6);
    const auto note = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.01, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{valuation_date, 90.0, 100.0}, {future_observation, 90.0, 0.2}, {expiry_date, 90.0, 0.3}}});
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12), 100.0,
        kiyosi::start_of_day(valuation_date) + std::chrono::hours{12},
        kiyosi::all_days_calendar());

    const auto result = kiyosi::make_finite_difference_autocallable_engine({100, 3}).value().price(note, context);

    REQUIRE(result);
    const double expected = 1.0 + 0.2 * *kiyosi::year_fraction(effective_date, future_observation);
    CHECK(*result == Catch::Approx(expected).epsilon(0.0).margin(1e-12));
}

TEST_CASE("Binary snowball finite difference prices active and knocked-out states")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const std::vector<kiyosi::Date> observation_dates{
        day(2025, 2, 3), day(2025, 6, 17), day(2025, 11, 5), expiry_date};
    const auto note = [&](kiyosi::AutocallableBarrierState barrier_state) {
        return *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.05, .barrier_state = barrier_state, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observation_dates, std::vector<double>{112.0, 108.0, 104.0, 100.0}, std::vector<double>{0.02, 0.04, 0.06, 0.08})});
    };
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 97.0, effective_date);

    for (const auto scheme : {kiyosi::FiniteDifferenceScheme::explicit_euler,
                              kiyosi::FiniteDifferenceScheme::implicit_euler,
                              kiyosi::FiniteDifferenceScheme::crank_nicolson}) {
        CAPTURE(scheme);
        const auto engine = kiyosi::make_finite_difference_autocallable_engine({80, 512, scheme, 500.0}).value();
        const auto untouched = engine.price(note(kiyosi::AutocallableBarrierState::none), context);
        const auto up_touched = engine.price(note(kiyosi::AutocallableBarrierState::knocked_out), context);

        REQUIRE(untouched);
        REQUIRE_FALSE(up_touched);
        CHECK(*untouched > 0.0);
        CHECK(up_touched.error().category == kiyosi::ErrorCategory::invalid_option);
    }
}

TEST_CASE("Autocallable engines enumerate dates only for daily monitoring")
{
    const auto valuation = day(2025, 1, 1);
    const auto first_observation = day(2025, 1, 4);
    const auto expiry_date = day(2025, 1, 6);
    const auto parameters = *kiyosi::make_bsm_parameters(0.03, 0.0, 0.2);
    const auto binary = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.02, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = valuation, .expiry_date = expiry_date, .observations = {{first_observation, 110.0, 0.10}, {expiry_date, 110.0, 0.20}}});
    const auto make_snowball = [&](kiyosi::KnockInObservationMode knock_in_observation_mode) {
        return *kiyosi::make_snowball_option({.maturity_coupon_rate = 0.02, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = knock_in_observation_mode, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = valuation, .expiry_date = expiry_date, .observations = {{first_observation, 110.0, 0.10}, {expiry_date, 110.0, 0.20}}});
    };
    const bool monte_carlo = GENERATE(false, true);
    CAPTURE(monte_carlo);
    const auto price = [&](const auto& note, const auto& market) {
        if (monte_carlo)
            return kiyosi::make_monte_carlo_autocallable_engine({32, 7}).value().price(note, market);
        return kiyosi::make_finite_difference_autocallable_engine({40, 40}).value().price(note, market);
    };
    const auto context = [&](kiyosi::TradingCalendar calendar) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(parameters, 100.0, valuation, std::move(calendar));
    };

    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto sparse_calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date) {
            ++*calls;
            return true;
        },
        365);
    const auto sparse_context = context(sparse_calendar);

    const auto binary_result = price(binary, sparse_context);
    REQUIRE(binary_result);
    CHECK(calls->load() == 3);

    calls->store(0);
    const auto expiry_only = make_snowball(kiyosi::KnockInObservationMode::at_expiry);
    const auto expiry_result = price(expiry_only, sparse_context);
    REQUIRE(expiry_result);
    CHECK(calls->load() == 3);

    calls->store(0);
    const auto daily_calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date) {
            ++*calls;
            return true;
        },
        365);
    const auto daily_context = context(daily_calendar);
    const auto daily_result = price(make_snowball(kiyosi::KnockInObservationMode::every_trading_day), daily_context);
    REQUIRE(daily_result);
    CHECK(calls->load() == 9);

    calls->store(0);
    const auto invalid_calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date value) {
            ++*calls;
            return value != expiry_date;
        },
        365);
    const auto invalid_context = context(invalid_calendar);
    const auto invalid = price(binary, invalid_context);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_date);
    CHECK(calls->load() == 2);
}

TEST_CASE("Finite-difference binary snowball engine rejects unstable explicit grids")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const std::vector<kiyosi::Date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                      day(2025, 10, 1), expiry_date};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto binary = *kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.08, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observation_dates, knock_outs, coupons)});
    CHECK_FALSE(kiyosi::make_finite_difference_autocallable_engine({40, 1, kiyosi::FiniteDifferenceScheme::explicit_euler}).value().price(binary, context));
}

TEST_CASE("Finite-difference phoenix engine rejects domains below the barrier")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const std::vector<kiyosi::Date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                      day(2025, 10, 1), expiry_date};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.02, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observation_dates, knock_outs, std::nullopt, std::vector<double>{90.0, 90.0, 90.0, 90.0})});
    CHECK_FALSE(kiyosi::make_finite_difference_autocallable_engine({40, 512, kiyosi::FiniteDifferenceScheme::crank_nicolson, 110.0}).value().price(phoenix, context));
}

} // namespace
