#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <future>
#include <limits>
#include <memory>
#include <optional>
#include <random>
#include <tuple>
#include <utility>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"
#include "pricing/engines/monte_carlo_random.hpp"
#include "pricing/engines/monte_carlo_host.hpp"

namespace {
using kiyosi::test::day;

TEST_CASE("Shared Monte Carlo Greeks propagate invalid batched sensitivities", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    const auto option = *make_european_option(OptionType::call, 100.0, start, end);
    const auto result = detail::complete_monte_carlo_greeks(
        AnalyticVanillaEngine{}, option, context, {Greek::rho}, {}, MonteCarloBackend::cpu,
        [](const PricingContext&) -> Result<void> { return {}; },
        []<std::size_t count>(const PricingContext&, double) -> Result<std::array<double, count>> { return std::array<double, count>{}; },
        [](double, std::optional<double>) -> Result<std::array<double, 3>> {
            const double large = std::numeric_limits<double>::max();
            return std::array{1.0, large, -large};
        });
    REQUIRE_FALSE(result);
    CHECK(result.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("Numerical accumulator Greeks reuse joint spot simulations", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(start) + std::chrono::hours{12}, calendar);
    const auto option = *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    const MonteCarloAccumulatorEngine engine{31, 42};
    queries->store(0);
    const auto scalar = detail::price_with_greeks(engine, option, context, true, {});
    REQUIRE(scalar);
    const int scalar_queries = queries->exchange(0);
    const auto result = calculate_numerical_greeks(engine, option, context);
    REQUIRE(result);
    CHECK(queries->load() < scalar_queries);
    CHECK(result->price() == scalar->price());
    CHECK(result->values_view() == scalar->values_view());
}

TEST_CASE("Accumulator spot and rate Greeks prepare the base trajectory once", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    int queries = 0;
    const auto calendar = *make_trading_calendar([&](Date) { ++queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.8), 100.0,
                                               start_of_day(start) + std::chrono::hours{12}, calendar);
    const auto option = *make_accumulator({.strike = 100.0, .knock_out_level = 110.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    const MonteCarloAccumulatorEngine engine{31, 42};
    for (const double shift : {0.2, 60.0, 200.0}) {
        CAPTURE(shift);
        const NumericalShiftSettings shifts{.spot_shift = shift, .rate_shift = 0.25};
        const auto expected = detail::price_with_greeks(engine, option, context, true, shifts);
        REQUIRE(expected);
        queries = 0;
        const auto rho = engine.price_with_greeks(option, context, {Greek::rho}, shifts);
        REQUIRE(rho);
        const int rate_queries = std::exchange(queries, 0);
        for (const auto request : {GreeksRequest{Greek::delta, Greek::gamma, Greek::rho},
                                   GreeksRequest{Greek::delta, Greek::gamma, Greek::speed, Greek::rho}}) {
            queries = 0;
            const auto actual = engine.price_with_greeks(option, context, request, shifts);
            REQUIRE(actual);
            CHECK(queries == rate_queries);
            CHECK(actual->price() == expected->price());
            CHECK(actual->values_view() == expected->selected(request).values_view());
        }
    }
}

TEST_CASE("Accumulator Monte Carlo joint spot Greeks preserve scalar seeded valuations", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    const auto option = *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 3.0, .effective_date = start, .expiry_date = end});
    const NumericalShiftSettings shifts{.spot_shift = 0.2};
    for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()})
        for (const int paths : {1, 31, 32})
            for (const double spot : {95.0, 100.0, 114.9, 116.0})
                for (const auto valuation : {start_of_day(start), start_of_day(start) + std::chrono::hours{12}, start_of_day(end)}) {
                    CAPTURE(seed, paths, spot, valuation.time_since_epoch().count());
                    const MonteCarloAccumulatorEngine engine{paths, seed};
                    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), spot, valuation, all_days_calendar());
                    const auto expected = detail::price_with_greeks(engine, option, context, true, shifts);
                    REQUIRE(expected);
                    const auto numerical = calculate_numerical_greeks(engine, option, context, shifts);
                    REQUIRE(numerical);
                    CHECK(numerical->price() == expected->price());
                    CHECK(numerical->values_view() == expected->values_view());
                    for (const GreeksRequest request : {GreeksRequest{true}, GreeksRequest{Greek::delta, Greek::gamma}, GreeksRequest{Greek::speed}, GreeksRequest{Greek::vanna}, GreeksRequest{Greek::zomma}, GreeksRequest{Greek::vega, Greek::vanna, Greek::zomma}}) {
                        const auto actual = engine.price_with_greeks(option, context, request, shifts);
                        REQUIRE(actual);
                        CHECK(actual->price() == expected->price());
                        CHECK(actual->values_view() == expected->selected(request).values_view());
                    }
                }
}

TEST_CASE("Accumulator Monte Carlo rate Greeks preserve independent seeded valuations", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    for (const bool fixed : {false, true}) {
        const auto option = *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = fixed ? 0.0 : 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 3.0, .effective_date = start, .expiry_date = end});
        const auto parameters = *make_bsm_parameters(fixed ? 0.0 : 0.04, fixed ? 0.0 : 0.01, 0.2);
        for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()})
            for (const int paths : {1, 31, 32})
                for (const double spot : {95.0, 114.9, 116.0})
                    for (const auto valuation : {start_of_day(start), start_of_day(start) + std::chrono::hours{12}, start_of_day(end)}) {
                        CAPTURE(fixed, seed, paths, spot, valuation.time_since_epoch().count());
                        const MonteCarloAccumulatorEngine engine{paths, seed};
                        const auto context = *make_pricing_context(parameters, spot, valuation, all_days_calendar());
                        for (const auto request : {GreeksRequest{Greek::rho}, GreeksRequest{Greek::delta, Greek::gamma, Greek::rho}}) {
                            const NumericalShiftSettings shifts{.rate_shift = 0.25};
                            const auto expected = detail::price_with_greeks(engine, option, context, request, shifts);
                            const auto actual = engine.price_with_greeks(option, context, request, shifts);
                            REQUIRE(expected);
                            REQUIRE(actual);
                            CHECK(actual->price() == expected->price());
                            CHECK(actual->values_view() == expected->values_view());
                        }
                    }
    }
    const auto option = *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start_of_day(start) + std::chrono::hours{12});
    const MonteCarloAccumulatorEngine engine{31, 42};
    const auto unavailable = engine.price_with_greeks(option, context, {Greek::rho}, {.rate_shift = 1e307});
    REQUIRE(unavailable);
    CHECK(unavailable->price() == *engine.price(option, context));
    CHECK_FALSE(unavailable->has(Greek::rho));
    const auto failed = engine.price_with_greeks(option, context, {Greek::rho}, {.rate_shift = 1e6});
    REQUIRE_FALSE(failed);
    CHECK(failed.error().category == ErrorCategory::invalid_result);
    CHECK(engine.price_with_greeks(option, context, {Greek::rho}));
}

TEST_CASE("Accumulator Monte Carlo joint valuations preserve failures and concurrent calls", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 13);
    const auto option = *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 3.0, .effective_date = start, .expiry_date = end});
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start);
    const MonteCarloAccumulatorEngine engine{127, 42};
    auto first = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, true); });
    auto second = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, true); });
    const auto a = first.get(), b = second.get();
    REQUIRE(a);
    REQUIRE(b);
    CHECK(a->price() == b->price());
    CHECK(a->values_view() == b->values_view());
    const auto bad_paths = MonteCarloAccumulatorEngine{0, 42}.price_with_greeks(option, context, true);
    REQUIRE_FALSE(bad_paths);
    CHECK(bad_paths.error().category == ErrorCategory::invalid_parameter);
    const auto bad_shifts = engine.price_with_greeks(option, context, true, {.spot_shift = 0.0});
    REQUIRE_FALSE(bad_shifts);
    CHECK(bad_shifts.error().category == ErrorCategory::invalid_parameter);
    const auto expired = *make_pricing_context(context.model_parameters(), 100.25, end + std::chrono::days{1});
    const auto bad_time = engine.price_with_greeks(option, expired, true);
    REQUIRE_FALSE(bad_time);
    CHECK(bad_time.error().category == ErrorCategory::invalid_time_range);
    const MonteCarloAccumulatorEngine unseeded{31, std::nullopt};
    const auto random = unseeded.price_with_greeks(option, context, true);
    REQUIRE(random);
    CHECK(random->all_finite());
    CHECK_FALSE(unseeded.settings().seed);
    const auto again = engine.price_with_greeks(option, context, true);
    REQUIRE(again);
    CHECK(again->values_view() == a->values_view());
}

TEST_CASE("Accumulator daily leap-year grids preserve settlement prices", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2024, 1, 1), end = day(2025, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, all_days_calendar());
    const auto option = *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 0.0, .effective_date = start, .expiry_date = end});
    // Prices from the original grid, including its redundant near-zero time steps.
    for (const auto& [scheme, nodes, expected] : {
             std::tuple{FiniteDifferenceScheme::crank_nicolson, 200, -2058.336718606278},
             std::tuple{FiniteDifferenceScheme::implicit_euler, 200, -2048.4071966441415},
             std::tuple{FiniteDifferenceScheme::explicit_euler, 40, -2253.4061717943714}}) {
        const auto price = FiniteDifferenceAccumulatorEngine{{nodes, 366, scheme}}.price(option, context);
        REQUIRE(price);
        CHECK(*price == Catch::Approx(expected).margin(1e-8));
    }
}

TEST_CASE("Zero-exposure accumulators skip numerical work after validation", "[fd-performance][monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto options = std::array{
        *make_accumulator({.strike = 100.0, .knock_out_level = 110.0, .daily_quantity = 0.0, .acceleration_factor = 2.0, .accumulated_quantity = 0.0, .effective_date = start, .expiry_date = end}),
        *make_accumulator({.strike = 100.0, .knock_out_level = 100.0, .daily_quantity = 1.0, .acceleration_factor = 0.0, .accumulated_quantity = 0.0, .effective_date = start, .expiry_date = end}),
        *make_accumulator({.strike = 110.0, .knock_out_level = 105.0, .daily_quantity = 1.0, .acceleration_factor = 0.0, .accumulated_quantity = 0.0, .effective_date = start, .expiry_date = end})};
    const auto parameters = *make_bsm_parameters(0.04, 0.01, 0.2);
    for (const auto& option : options)
        for (const auto valuation : {start_of_day(start), start_of_day(start) + std::chrono::hours{12}}) {
            const auto context = *make_pricing_context(parameters, 90.0, valuation, calendar);
            const auto check = [&](const auto& engine) {
                queries->store(0);
                const auto price = engine.price(option, context);
                REQUIRE(price);
                CHECK(*price == 0.0);
                CHECK(queries->load() < 3);
                const auto greeks = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::vega, Greek::rho});
                REQUIRE(greeks);
                CHECK(greeks->price() == 0.0);
                for (const auto greek : {Greek::delta, Greek::gamma, Greek::vega, Greek::rho})
                    CHECK(test::greek_value(*greeks, greek) == 0.0);
                const auto expired = *make_pricing_context(parameters, 90.0, end + std::chrono::days{1}, calendar);
                const auto invalid = engine.price(option, expired);
                REQUIRE_FALSE(invalid);
                CHECK(invalid.error().category == ErrorCategory::invalid_time_range);
            };
            check(MonteCarloAccumulatorEngine{2'000, 42});
            check(MonteCarloAccumulatorEngine{2'000, 42, MonteCarloBackend::cuda});
            check(FiniteDifferenceAccumulatorEngine{{.asset_upper_boundary = 1.0}});
            const auto invalid_mc = MonteCarloAccumulatorEngine{0, 42}.price(option, context);
            REQUIRE_FALSE(invalid_mc);
            CHECK(invalid_mc.error().category == ErrorCategory::invalid_parameter);
            const auto invalid_fd = FiniteDifferenceAccumulatorEngine{{.time_step_count = 0}}.price(option, context);
            REQUIRE_FALSE(invalid_fd);
            CHECK(invalid_fd.error().category == ErrorCategory::invalid_parameter);
        }
}

TEST_CASE("Accumulator finite-difference spot Greeks reuse one solve", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 70.0,
                                               start_of_day(start) + std::chrono::hours{12}, calendar);
    const auto option = *make_accumulator(
        {.strike = 50.0, .knock_out_level = 200.0, .daily_quantity = 0.0, .acceleration_factor = 1.0, .accumulated_quantity = 3.0, .effective_date = start, .expiry_date = end});
    const FiniteDifferenceAccumulatorEngine engine{};
    const auto price = engine.price(option, context);
    REQUIRE(price);
    const int price_queries = queries->exchange(0);
    const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::speed});
    REQUIRE(result);
    CHECK(queries->load() == price_queries);
    CHECK(result->price() == *price);
    CHECK(result->has(Greek::speed));
    const double time = *year_fraction(context.valuation_time(), start_of_day(end));
    CHECK(test::greek_value(*result, Greek::delta) == Catch::Approx(3.0 * std::exp(-0.01 * time)).margin(1e-7));
    CHECK(test::greek_value(*result, Greek::gamma) == Catch::Approx(0.0).margin(1e-10));
    CHECK_FALSE(result->has(Greek::vega));
}

TEST_CASE("Accumulator fixed-quantity finite differences preserve accrual linearity", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0,
                                               start_of_day(start) + std::chrono::hours{12});
    const auto option = [&](double daily, double accumulated) {
        return *make_accumulator({.strike = 90.0, .knock_out_level = 110.0, .daily_quantity = daily, .acceleration_factor = 2.0, .accumulated_quantity = accumulated, .effective_date = start, .expiry_date = end});
    };
    for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler,
                              FiniteDifferenceScheme::crank_nicolson}) {
        CAPTURE(scheme);
        const FiniteDifferenceAccumulatorEngine engine{{.asset_step_count = 40, .time_step_count = 200, .scheme = scheme, .asset_upper_boundary = 440.0}};
        const auto fixed = engine.price_with_greeks(option(0.0, 3.0), context, {Greek::delta, Greek::gamma});
        const auto with_purchases = engine.price_with_greeks(option(1.0, 3.0), context, {Greek::delta, Greek::gamma});
        const auto purchases_only = engine.price_with_greeks(option(1.0, 0.0), context, {Greek::delta, Greek::gamma});
        REQUIRE(fixed);
        REQUIRE(with_purchases);
        REQUIRE(purchases_only);
        CHECK(fixed->price() == Catch::Approx(with_purchases->price() - purchases_only->price()).margin(1e-9));
        for (const auto greek : {Greek::delta, Greek::gamma})
            CHECK(test::greek_value(*fixed, greek) ==
                  Catch::Approx(test::greek_value(*with_purchases, greek) - test::greek_value(*purchases_only, greek)).margin(1e-10));
    }
}

TEST_CASE("Inactive accumulator accrual preserves fixed-position FD values", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0,
                                               start_of_day(start) + std::chrono::hours{12});
    for (const double strike : {105.0, 110.0}) {
        const auto option = [&](double daily) {
            return *make_accumulator({.strike = strike, .knock_out_level = 105.0, .daily_quantity = daily, .acceleration_factor = 0.0, .accumulated_quantity = 3.0, .effective_date = start, .expiry_date = end});
        };
        for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler,
                                  FiniteDifferenceScheme::crank_nicolson}) {
            CAPTURE(strike, scheme);
            const FiniteDifferenceAccumulatorEngine engine{{.asset_step_count = 40, .time_step_count = 200, .scheme = scheme, .asset_upper_boundary = 440.0}};
            const auto inactive = engine.price_with_greeks(option(1.0), context, {Greek::delta, Greek::gamma});
            const auto fixed = engine.price_with_greeks(option(0.0), context, {Greek::delta, Greek::gamma});
            REQUIRE(inactive);
            REQUIRE(fixed);
            CHECK(inactive->price() == fixed->price());
            for (const auto greek : {Greek::delta, Greek::gamma})
                CHECK(test::greek_value(*inactive, greek) == test::greek_value(*fixed, greek));
        }
    }
}

double reference_accumulator_price(const kiyosi::Accumulator& option,
                                   const kiyosi::PricingContext& context,
                                   kiyosi::TradingDayMonteCarloSettings settings)
{
    const auto path_payoff = [&](kiyosi::detail::Pcg32& generator) {
        const double rate = context.model_parameters().risk_free_rate();
        const double dividend = context.model_parameters().dividend_yield();
        const double sigma = context.model_parameters().volatility();
        const auto valuation = context.valuation_time();

        double value = context.spot_price();
        double quantity = option.accumulated_quantity();
        double terminal = value;
        if (valuation == kiyosi::start_of_day(kiyosi::date_of(valuation)) &&
            context.calendar().is_trading_day(kiyosi::date_of(valuation))) {
            if (value >= option.knock_out_level()) return quantity * (value - option.strike());
            quantity += value < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                                : option.daily_quantity();
        }
        if (valuation == option.expiry_date()) return quantity * (value - option.strike());

        std::normal_distribution<double> normal;
        auto previous = valuation;
        for (auto current = kiyosi::date_of(valuation) + std::chrono::days{1};
             current <= option.expiry_date(); current += std::chrono::days{1}) {
            if (!context.calendar().is_trading_day(current)) continue;
            const double dt = *kiyosi::year_fraction(previous, kiyosi::start_of_day(current));
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt +
                              sigma * std::sqrt(dt) * normal(generator));
            previous = kiyosi::start_of_day(current);
            if (value >= option.knock_out_level()) {
                terminal = value;
                break;
            }
            quantity += value < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                                : option.daily_quantity();
            terminal = value;
        }
        return quantity * (terminal - option.strike()) *
               std::exp(-rate * *kiyosi::year_fraction(valuation, previous));
    };

    std::mt19937_64 generator(settings.seed.value_or(0));
    double sum = 0.0;
    for (int path = 0; path < settings.path_count; ++path) {
        kiyosi::detail::Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
        sum += path_payoff(path_generator);
    }
    return sum / static_cast<double>(settings.path_count);
}
} // namespace

TEST_CASE("Fixed-position accumulators use exact zero-carry values", "[fd-performance][monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    for (const double strike : {50.0, 105.0, 110.0}) {
        const auto option = *make_accumulator({.strike = strike, .knock_out_level = 105.0, .daily_quantity = strike == 50.0 ? 0.0 : 1.0, .acceleration_factor = 0.0, .accumulated_quantity = 3.0, .effective_date = start, .expiry_date = end});
        for (const auto valuation : {start_of_day(start), start_of_day(start) + std::chrono::hours{12}}) {
            const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 100.0, valuation, calendar);
            const auto check = [&](const auto& engine) {
                queries->store(0);
                const auto price = engine.price(option, context);
                REQUIRE(price);
                CHECK(*price == 3.0 * (100.0 - strike));
                CHECK(queries->load() <= 2);
                const auto greeks = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                REQUIRE(greeks);
                CHECK(test::greek_value(*greeks, Greek::delta) == Catch::Approx(3.0).margin(1e-9));
                CHECK(test::greek_value(*greeks, Greek::gamma) == Catch::Approx(0.0).margin(1e-8));
            };
            check(MonteCarloAccumulatorEngine{32, 7});
            check(MonteCarloAccumulatorEngine{2'000, 42, MonteCarloBackend::cuda});
            check(FiniteDifferenceAccumulatorEngine{});
            const auto invalid_mc = MonteCarloAccumulatorEngine{0, 42}.price(option, context);
            REQUIRE_FALSE(invalid_mc);
            CHECK(invalid_mc.error().category == ErrorCategory::invalid_parameter);
            const auto invalid_fd = FiniteDifferenceAccumulatorEngine{{.time_step_count = 0}}.price(option, context);
            REQUIRE_FALSE(invalid_fd);
            CHECK(invalid_fd.error().category == ErrorCategory::invalid_parameter);
        }
    }
}

TEST_CASE("Accumulator exact-value shortcut requires zero carry and no accrual")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 6);
    const TradingDayMonteCarloSettings settings{32, 7};
    for (const auto [rate, dividend, daily] : std::array{
             std::array{0.04, 0.0, 0.0}, std::array{0.0, 0.01, 0.0},
             std::array{0.04, 0.04, 0.0}, std::array{0.0, 0.0, 1.0}}) {
        const auto option = *make_accumulator({.strike = 100.0, .knock_out_level = 110.0, .daily_quantity = daily, .acceleration_factor = 2.0, .accumulated_quantity = 3.0, .effective_date = start, .expiry_date = end});
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, dividend, 0.2),
                                                   90.0, start_of_day(start) + std::chrono::hours{12});
        const auto price = MonteCarloAccumulatorEngine{settings}.price(option, context);
        REQUIRE(price);
        CHECK(*price == Catch::Approx(reference_accumulator_price(option, context, settings)).margin(1e-12));
        CHECK(*price != -30.0);
    }
}

TEST_CASE("Accumulator expiry_date settlement agrees across pricing engines")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), spot, expiry_date);
    };
    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out_level = 110.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration_factor = 2.0,
                                                        .accumulated_quantity = 3.0,
                                                        .effective_date = effective_date,
                                                        .expiry_date = expiry_date});

    const auto monte_carlo = kiyosi::MonteCarloAccumulatorEngine{{32, 7}}.price(accumulator, market(90.0));
    REQUIRE(monte_carlo);
    CHECK(*monte_carlo == Catch::Approx(-50.0));

    const auto finite_difference =
        kiyosi::FiniteDifferenceAccumulatorEngine{}.price(accumulator, market(90.0));
    REQUIRE(finite_difference);
    CHECK(*finite_difference == Catch::Approx(-50.0));
}

TEST_CASE("Accumulator engines reject nominal weekend expiry and accept adjusted expiry")
{
    const auto nominal = day(2025, 1, 5);
    const auto calendar = kiyosi::weekdays_calendar();
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.01, 0.0, 0.2), 110.0, day(2025, 1, 3), calendar);
    const auto option = [&](kiyosi::Date expiry) {
        return *kiyosi::make_accumulator({.strike = 100.0, .knock_out_level = 1000.0, .daily_quantity = 0.0, .acceleration_factor = 1.0, .accumulated_quantity = 1.0, .effective_date = day(2025, 1, 3), .expiry_date = expiry});
    };
    const auto unadjusted = option(nominal);
    const auto adjusted_expiry = calendar.adjust(nominal, kiyosi::BusinessDayConvention::following);
    REQUIRE(adjusted_expiry);
    const auto adjusted = option(*adjusted_expiry);
    const kiyosi::MonteCarloAccumulatorEngine mc{{32, 7}};
    const kiyosi::FiniteDifferenceAccumulatorEngine fd{};
    for (const auto& result : {mc.price(unadjusted, context), fd.price(unadjusted, context)}) {
        REQUIRE_FALSE(result);
        CHECK(result.error().category == kiyosi::ErrorCategory::invalid_date);
    }
    CHECK(mc.price(adjusted, context).has_value());
    CHECK(fd.price(adjusted, context).has_value());
}

TEST_CASE("Accumulator Monte Carlo prepares stable calendar inputs once")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = day(2025, 1, 6);
    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date value) {
            ++*calls;
            return value == valuation || value == day(2025, 1, 2) ||
                   value == day(2025, 1, 4) || value == expiry_date;
        },
        252);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.0, 0.2), 100.0, valuation, calendar);
    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out_level = 101.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration_factor = 2.0,
                                                        .accumulated_quantity = 3.0,
                                                        .effective_date = valuation,
                                                        .expiry_date = expiry_date});
    const std::array seeds{std::uint64_t{0}, std::numeric_limits<std::uint64_t>::max()};
    for (const std::uint64_t seed : seeds) {
        CAPTURE(seed);
        const kiyosi::TradingDayMonteCarloSettings settings{32, seed};
        const double expected = reference_accumulator_price(accumulator, context, settings);
        calls->store(0);

        const auto result = kiyosi::MonteCarloAccumulatorEngine{settings}.price(accumulator, context);

        REQUIRE(result);
        CHECK(*result == Catch::Approx(expected).epsilon(0.0).margin(1e-12));
        CHECK(calls->load() == 7);
    }

    const auto unseeded =
        kiyosi::MonteCarloAccumulatorEngine{{32, std::nullopt}}.price(accumulator, context);
    REQUIRE(unseeded);
    CHECK(std::isfinite(*unseeded));
}

TEST_CASE("Accumulator Monte Carlo settles deterministic states before simulation")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = day(2025, 1, 6);
    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date) {
            ++*calls;
            return true;
        },
        365);
    const auto context = [&](kiyosi::Date date, double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), spot, date, calendar);
    };
    const auto make_option = [&](double accumulated_quantity) {
        return *kiyosi::make_accumulator({.strike = 100.0,
                                          .knock_out_level = 101.0,
                                          .daily_quantity = 0.0,
                                          .acceleration_factor = 2.0,
                                          .accumulated_quantity = accumulated_quantity,
                                          .effective_date = valuation,
                                          .expiry_date = expiry_date});
    };

    const double large_payoff = std::numeric_limits<double>::max() / 64.0;
    const auto immediate = kiyosi::MonteCarloAccumulatorEngine{{128, 7}}.price(
        make_option(large_payoff), context(valuation, 101.0));
    REQUIRE(immediate);
    CHECK(*immediate == large_payoff);
    CHECK(calls->load() == 2);

    calls->store(0);
    const double expiry_quantity = std::numeric_limits<double>::max() / 640.0;
    const double expiry_payoff = expiry_quantity * -10.0;
    const auto at_expiry = kiyosi::MonteCarloAccumulatorEngine{{128, 7}}.price(
        make_option(expiry_quantity), context(expiry_date, 90.0));
    REQUIRE(at_expiry);
    CHECK(*at_expiry == expiry_payoff);
    CHECK(calls->load() == 2);

    calls->store(0);
    const auto invalid_settings = kiyosi::MonteCarloAccumulatorEngine{{0, 7}}.price(
        make_option(1.0), context(valuation, 101.0));
    REQUIRE_FALSE(invalid_settings);
    CHECK(invalid_settings.error().category == kiyosi::ErrorCategory::invalid_parameter);
    CHECK(calls->load() == 0);

    const auto non_finite = kiyosi::MonteCarloAccumulatorEngine{{128, 7}}.price(
        make_option(std::numeric_limits<double>::max()), context(valuation, 102.0));
    REQUIRE_FALSE(non_finite);
    CHECK(non_finite.error().category == kiyosi::ErrorCategory::invalid_result);
}

TEST_CASE("Accumulator CUDA selection validates and preserves deterministic settlements")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = day(2025, 1, 6);
    const auto option = *kiyosi::make_accumulator({.strike = 100.0,
                                                   .knock_out_level = 110.0,
                                                   .daily_quantity = 1.0,
                                                   .acceleration_factor = 2.0,
                                                   .accumulated_quantity = 3.0,
                                                   .effective_date = valuation,
                                                   .expiry_date = expiry_date});
    const auto context = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.03, 0.01, 0.2), spot, valuation,
            kiyosi::all_days_calendar());
    };
    const kiyosi::MonteCarloAccumulatorEngine cuda{{64, 7,
                                                    kiyosi::MonteCarloBackend::cuda}};
    CHECK(cuda.settings().backend == kiyosi::MonteCarloBackend::cuda);

    const auto settled = cuda.price(option, context(110.0));
    REQUIRE(settled);
    CHECK(*settled == 30.0);

    const auto invalid = kiyosi::MonteCarloAccumulatorEngine{
        {0, 7, kiyosi::MonteCarloBackend::cuda}}
                             .price(option, context(100.0));
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto invalid_backend =
        kiyosi::MonteCarloAccumulatorEngine{
            {64, 7, static_cast<kiyosi::MonteCarloBackend>(255)}}
            .price(option, context(110.0));
    REQUIRE_FALSE(invalid_backend);
    CHECK(invalid_backend.error().category == kiyosi::ErrorCategory::invalid_parameter);

#if !KIYOSI_HAS_CUDA
    const auto unavailable = cuda.price(option, context(100.0));
    REQUIRE_FALSE(unavailable);
    CHECK(unavailable.error().category == kiyosi::ErrorCategory::backend_unavailable);
#endif
}

#if KIYOSI_HAS_CUDA
TEST_CASE("Accumulator CUDA Monte Carlo preserves accrual and seeded execution")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2025, 1, 4);
    const auto option = *kiyosi::make_accumulator({.strike = 100.0,
                                                   .knock_out_level = 200.0,
                                                   .daily_quantity = 1.0,
                                                   .acceleration_factor = 2.0,
                                                   .accumulated_quantity = 3.0,
                                                   .effective_date = effective_date,
                                                   .expiry_date = expiry_date});
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-8), 90.0, effective_date,
        kiyosi::all_days_calendar());
    const kiyosi::MonteCarloAccumulatorEngine engine{
        {32'768, 73, kiyosi::MonteCarloBackend::cuda}};

    auto first_future = std::async(std::launch::async, [&] { return engine.price(option, context); });
    auto second_future = std::async(std::launch::async, [&] { return engine.price(option, context); });
    const auto first = first_future.get();
    const auto second = second_future.get();

    REQUIRE(first);
    REQUIRE(second);
    const double first_price = *first;
    CHECK(first_price == *second);
    CHECK(first_price == Catch::Approx(-110.0).epsilon(0.0).margin(1e-5));
}
#endif
