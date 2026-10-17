#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <atomic>
#include <chrono>
#include <future>
#include <limits>
#include <memory>
#include <numbers>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"
#include "support/numerical.hpp"

namespace {

using kiyosi::test::day;

TEST_CASE("Analytic barriers preserve small-volatility prices with nonzero carry", "[barrier-carry]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const double sigma : {1e-160, 1e-200})
        for (const double carry : {-0.3, -0.02, 0.02, 0.3})
            for (const bool up : {false, true})
                for (const bool knock_in : {false, true})
                    for (const auto direction : {OptionType::call, OptionType::put}) {
                        CAPTURE(sigma, carry, up, knock_in, direction);
                        const double terminal = 100.0 * std::exp(carry);
                        const bool touched = up ? terminal >= 120.0 : terminal <= 80.0;
                        const double payoff = std::max((direction == OptionType::call ? 1.0 : -1.0) * (terminal - 100.0), 0.0);
                        const double expected = (knock_in == touched ? payoff : 7.0) * std::exp(-0.03);
                        const auto context = *make_pricing_context(*make_bsm_parameters(0.03, 0.03 - carry, sigma), 100.0, start);
                        const auto option = *make_barrier_option({.option_type = direction, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 120.0 : 80.0, .barrier_type = up ? (knock_in ? BarrierType::up_and_in : BarrierType::up_and_out) : (knock_in ? BarrierType::down_and_in : BarrierType::down_and_out), .rebate = 7.0, .rebate_timing = RebateTiming::at_expiry});
                        const auto actual = AnalyticBarrierEngine{}.price(option, context);
                        REQUIRE(actual);
                        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(expected, 1e-11));
                    }
}

TEST_CASE("Equivalent hit rebates and touch payments share discount validation", "[hit-discount]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const bool up : {false, true}) {
        const double level = up ? 120.0 : 80.0;
        const auto barrier = *make_barrier_option({.option_type = up ? OptionType::call : OptionType::put,
                                                   .strike = level,
                                                   .effective_date = start,
                                                   .expiry_date = end,
                                                   .barrier_level = level,
                                                   .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out,
                                                   .rebate = 7.0,
                                                   .rebate_timing = RebateTiming::at_hit});
        const auto touch = *(up ? make_cash_one_touch_up(start, end, level, 7.0, SettlementTiming::at_hit)
                                : make_cash_one_touch_down(start, end, level, 7.0, SettlementTiming::at_hit));
        for (const double dividend : {-0.04, -0.08, -0.08000000000000002, -0.1}) {
            CAPTURE(up, dividend);
            const auto context = *make_pricing_context(*make_bsm_parameters(-0.02, dividend, 0.2), 100.0, start);
            const auto rebate = AnalyticBarrierEngine{}.price(barrier, context);
            const auto payment = AnalyticBinaryBarrierEngine{}.price(touch, context);
            if (dividend == -0.1) {
                REQUIRE(rebate);
                REQUIRE(payment);
                CHECK_THAT(*rebate, Catch::Matchers::WithinAbs(*payment, 1e-12));
                CHECK(*payment > 0.0);
            } else {
                REQUIRE_FALSE(rebate);
                REQUIRE_FALSE(payment);
                CHECK(rebate.error().category == ErrorCategory::invalid_result);
                CHECK(payment.error().category == ErrorCategory::invalid_result);
            }
        }
    }
}

TEST_CASE("Scheduled analytic Greeks validate calendars once per request", "[analytic-schedule]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2035, 1, 1);
    std::vector<Date> dates;
    for (auto date = start + std::chrono::days{1}; date <= end; date += std::chrono::days{1})
        if (weekdays_calendar().is_trading_day(date)) dates.push_back(date);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto allowed = std::make_shared<std::atomic<bool>>(true);
    const auto calendar = *make_trading_calendar([queries, allowed](Date date) {
        ++*queries;
        return *allowed && weekdays_calendar().is_trading_day(date);
    },
                                                 252);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(start) + std::chrono::hours{12}, calendar);
    const auto check = [&](const auto& engine, const auto& option) {
        const auto expected = kiyosi::test::price_differences(engine, option, context);
        REQUIRE(expected);
        *queries = 0;
        const auto result = engine.price_with_greeks(option, context, true);
        REQUIRE(result);
        CHECK(result->price() == expected->price());
        CHECK(result->values_view() == expected->values_view());
        CHECK(*queries == dates.size());
        *queries = 0;
        auto task = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, true); });
        const auto concurrent = engine.price_with_greeks(option, context, true);
        const auto first = task.get();
        REQUIRE(first);
        REQUIRE(concurrent);
        CHECK(first->values_view() == result->values_view());
        CHECK(concurrent->values_view() == result->values_view());
        CHECK(*queries == 2 * dates.size());
        *allowed = false;
        const auto rejected = engine.price_with_greeks(option, context, true);
        REQUIRE_FALSE(rejected);
        CHECK(rejected.error().category == ErrorCategory::invalid_date);
        *allowed = true;
        *queries = 0;
        REQUIRE(engine.price_with_greeks(option, context, true));
        CHECK(*queries == dates.size());
        *queries = 0;
        REQUIRE_FALSE(engine.price_with_greeks(option, context, true, {.spot_shift = 0.0}));
        CHECK(*queries == 0);

        const auto quote = engine.price(option, context);
        REQUIRE(quote);
        const ImpliedVolatilitySettings bounds{.lower_bound = 0.15, .upper_bound = 0.25};
        *queries = 0;
        const auto solved = implied_volatility(engine, option, context, *quote, bounds);
        REQUIRE(solved);
        CHECK_THAT(*solved, Catch::Matchers::WithinAbs(0.2, 1e-7));
        CHECK(*queries == dates.size());
        *queries = 0;
        auto solve_task = std::async(std::launch::async, [&] { return implied_volatility(engine, option, context, *quote, bounds); });
        const auto concurrent_solve = implied_volatility(engine, option, context, *quote, bounds);
        const auto first_solve = solve_task.get();
        REQUIRE(first_solve);
        REQUIRE(concurrent_solve);
        CHECK(*first_solve == *solved);
        CHECK(*concurrent_solve == *solved);
        CHECK(*queries == 2 * dates.size());
        *allowed = false;
        const auto rejected_solve = implied_volatility(engine, option, context, *quote, bounds);
        REQUIRE_FALSE(rejected_solve);
        CHECK(rejected_solve.error().category == ErrorCategory::invalid_date);
        *allowed = true;
        *queries = 0;
        REQUIRE(implied_volatility(engine, option, context, *quote, bounds));
        CHECK(*queries == dates.size());
        *queries = 0;
        REQUIRE_FALSE(implied_volatility(engine, option, context, *quote, {.lower_bound = 0.0}));
        CHECK(*queries == 0);
    };
    check(AnalyticBarrierEngine{}, *make_barrier_option(
                                       {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = dates, .touch_state = BarrierTouchState::untouched}));
    const BinaryBarrierTerms terms{.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = dates, .touch_state = BarrierTouchState::untouched};
    check(AnalyticBinaryBarrierEngine{}, *make_cash_binary_barrier_option(terms, 10.0));
    check(AnalyticBinaryBarrierEngine{}, *make_asset_binary_barrier_option(terms));
    check(AnalyticBinaryBarrierEngine{}, *make_cash_no_touch_down(start, end, 80.0, 10.0, ObservationMode::scheduled, dates, BarrierTouchState::untouched));
    check(AnalyticBinaryBarrierEngine{}, *make_asset_one_touch_down(start, end, 80.0, SettlementTiming::at_expiry, ObservationMode::scheduled, dates, BarrierTouchState::untouched));
}

TEST_CASE("Analytic barrier branches preserve in-out parity and rebate timing", "[barrier-factors]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    const AnalyticBarrierEngine engine;
    for (const bool up : {false, true}) {
        for (const auto direction : {OptionType::call, OptionType::put}) {
            for (const double strike : {70.0, 80.0, 100.0, 120.0, 130.0}) {
                const auto vanilla = AnalyticVanillaEngine{}.price(*make_european_option(direction, strike, start, end), context);
                REQUIRE(vanilla);
                for (const double rebate : {0.0, 5.0}) {
                    for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
                        CAPTURE(up, direction, strike, rebate, timing);
                        BarrierOptionTerms terms{.option_type = direction, .strike = strike, .effective_date = start, .expiry_date = end, .barrier_level = up ? 120.0 : 80.0, .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out, .rebate = rebate, .rebate_timing = timing};
                        const auto out = engine.price(*make_barrier_option(terms), context);
                        terms.barrier_type = up ? BarrierType::up_and_in : BarrierType::down_and_in;
                        terms.rebate_timing = RebateTiming::at_expiry;
                        terms.rebate = timing == RebateTiming::at_hit ? 0.0 : rebate;
                        const auto in = engine.price(*make_barrier_option(terms), context);
                        REQUIRE(out);
                        REQUIRE(in);
                        double extra = rebate * std::exp(-0.04);
                        if (timing == RebateTiming::at_hit) {
                            const auto touch = (up ? make_cash_one_touch_up : make_cash_one_touch_down)(
                                start, end, terms.barrier_level, 1.0, SettlementTiming::at_hit, ObservationMode::continuous, {}, std::nullopt);
                            REQUIRE(touch);
                            const auto hit_rebate = AnalyticBinaryBarrierEngine{}.price(*touch, context);
                            REQUIRE(hit_rebate);
                            extra = rebate * *hit_rebate;
                        }
                        CHECK_THAT(*in + *out, Catch::Matchers::WithinAbs(*vanilla + extra, 1e-10));
                    }
                }
            }
        }
    }
}

TEST_CASE("Analytic barriers retain tiny positive volatility time value", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const AnalyticBarrierEngine engine;
    for (const auto type : {BarrierType::down_and_in, BarrierType::down_and_out,
                            BarrierType::up_and_in, BarrierType::up_and_out}) {
        const bool up = type == BarrierType::up_and_in || type == BarrierType::up_and_out;
        const bool knock_in = type == BarrierType::up_and_in || type == BarrierType::down_and_in;
        for (const auto direction : {OptionType::call, OptionType::put}) {
            for (const double spot : {1e16, 1e150}) {
                for (const double sigma : {1e-12, 1e-16, 1e-20, 1e-200}) {
                    CAPTURE(type, direction, spot, sigma);
                    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, sigma), spot, start);
                    const auto option = *make_barrier_option(
                        {.option_type = direction, .strike = spot, .effective_date = start, .expiry_date = end, .barrier_level = (up ? 1.2 : 0.8) * spot, .barrier_type = type});
                    const auto price = engine.price(option, context);
                    REQUIRE(price);
                    // Barrier-hit probabilities are negligible; knock-outs retain the vanilla time value.
                    const double expected = knock_in ? 0.0 : spot * sigma / std::sqrt(2.0 * std::numbers::pi);
                    CHECK_THAT(*price, Catch::Matchers::WithinRel(expected, 1e-12));
                    const auto joint = engine.price_with_greeks(option, context, {Greek::delta});
                    REQUIRE(joint);
                    CHECK(joint->price() == *price);
                }
            }
        }
    }
}

TEST_CASE("Analytic barrier prices preserve finite extreme monetary scales")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.05, 0.02, 0.2);
    for (const auto type : {BarrierType::down_and_in, BarrierType::down_and_out,
                            BarrierType::up_and_in, BarrierType::up_and_out}) {
        const bool up = type == BarrierType::up_and_in || type == BarrierType::up_and_out;
        for (const auto direction : {OptionType::call, OptionType::put}) {
            const auto prices = [&](double scale) {
                const BinaryBarrierTerms terms{
                    .option_type = direction, .strike = 100.0 * scale, .effective_date = start, .expiry_date = end, .barrier_level = (up ? 120.0 : 80.0) * scale, .barrier_type = type};
                const auto context = *make_pricing_context(parameters, 100.0 * scale, start);
                const auto vanilla = AnalyticBarrierEngine{}.price(*make_barrier_option(
                                                                       {.option_type = direction, .strike = terms.strike, .effective_date = start, .expiry_date = end, .barrier_level = terms.barrier_level, .barrier_type = type}),
                                                                   context);
                const auto cash = AnalyticBinaryBarrierEngine{}.price(*make_cash_binary_barrier_option(terms, 10.0), context);
                const auto asset = AnalyticBinaryBarrierEngine{}.price(*make_asset_binary_barrier_option(terms), context);
                REQUIRE(vanilla);
                REQUIRE(cash);
                REQUIRE(asset);
                return std::array{*vanilla / scale, *cash, *asset / scale};
            };
            const auto base = prices(1.0);
            for (const double scale : {1e155, 1e-170, 1e-200}) {
                CAPTURE(type, direction, scale);
                const auto scaled = prices(scale);
                for (std::size_t index = 0; index < base.size(); ++index)
                    CHECK_THAT(scaled[index], Catch::Matchers::WithinAbs(base[index], 1e-9));
            }
        }
    }
}

TEST_CASE("Analytic barriers settle at expiry for every touch and knock state")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2), 100.0, expiry);
    for (const auto type : {kiyosi::BarrierType::up_and_in, kiyosi::BarrierType::up_and_out}) {
        for (const auto state : {kiyosi::BarrierTouchState::untouched, kiyosi::BarrierTouchState::touched}) {
            for (const double level : {90.0, 120.0}) {
                for (const auto timing : {kiyosi::RebateTiming::at_hit, kiyosi::RebateTiming::at_expiry}) {
                    if (type == kiyosi::BarrierType::up_and_in && timing == kiyosi::RebateTiming::at_hit)
                        continue;
                    CAPTURE(type, state, level, timing);
                    const auto option = kiyosi::make_barrier_option(
                        {.option_type = kiyosi::OptionType::call,
                         .strike = 80.0,
                         .effective_date = effective,
                         .expiry_date = expiry,
                         .barrier_level = level,
                         .barrier_type = type,
                         .rebate = 7.0,
                         .rebate_timing = timing,
                         .touch_state = state});
                    REQUIRE(option);
                    const bool prior_touch = state == kiyosi::BarrierTouchState::touched;
                    const bool touched = prior_touch || level <= 100.0;
                    const double unpaid_rebate = prior_touch && timing == kiyosi::RebateTiming::at_hit ? 0.0 : 7.0;
                    const double expected = type == kiyosi::BarrierType::up_and_in
                                                ? (touched ? 20.0 : 7.0)
                                                : (touched ? unpaid_rebate : 20.0);
                    const auto result = kiyosi::AnalyticBarrierEngine{}.price(*option, context);
                    REQUIRE(result);
                    CHECK(*result == expected);
                }
            }
        }
    }
}

TEST_CASE("Barrier expiry settlement precedes rebate discounting", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto market = *make_pricing_context(*make_bsm_parameters(-0.02, -0.04, 0.2), 100.0, end);
    const auto option = *make_barrier_option({.option_type = OptionType::call,
                                              .strike = 80.0,
                                              .effective_date = start,
                                              .expiry_date = end,
                                              .barrier_level = 120.0,
                                              .barrier_type = BarrierType::up_and_out,
                                              .rebate = 7.0,
                                              .rebate_timing = RebateTiming::at_hit,
                                              .touch_state = BarrierTouchState::untouched});
    const auto check = [&](const auto& engine) {
        const auto scalar = engine.price(option, market);
        REQUIRE(scalar);
        CHECK(*scalar == 20.0);
        const auto joint = engine.price_with_greeks(option, market, true);
        REQUIRE(joint);
        CHECK(joint->price() == 20.0);
        CHECK_FALSE(joint->has(Greek::gamma));
    };
    check(AnalyticBarrierEngine{});
    check(FiniteDifferenceBarrierEngine{});
}

TEST_CASE("Already-hit barrier rebates respect expiry_date payment timing")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 100.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 90.0,
                                                       .barrier_type = kiyosi::BarrierType::up_and_out,
                                                       .rebate = 10.0,
                                                       .rebate_timing = kiyosi::RebateTiming::at_expiry});

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE(result.has_value());
    CHECK_THAT(*result, Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05), 1e-12));
}

TEST_CASE("Barrier hit rebates use the finite first-hit payment decomposition")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.03, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 1'000'000'000.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 110.0,
                                                       .barrier_type = kiyosi::BarrierType::up_and_out,
                                                       .rebate = 10.0,
                                                       .rebate_timing = kiyosi::RebateTiming::at_hit});

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE(result.has_value());
    const double distance = std::log(1.1);
    const double root = std::sqrt(2.0 * 0.05 * 0.2 * 0.2);
    const auto normal_cdf = [](double value) { return 0.5 * std::erfc(-value / std::numbers::sqrt2); };
    const double discounted_hit = std::exp(-root * distance / (0.2 * 0.2)) *
                                      normal_cdf((root - distance) / 0.2) +
                                  std::exp(root * distance / (0.2 * 0.2)) *
                                      normal_cdf((-root - distance) / 0.2);
    CHECK_THAT(*result, Catch::Matchers::WithinAbs(10.0 * discounted_hit, 1e-6));
}

TEST_CASE("Barrier hit rebates reject an unstable negative-rate limit")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(-0.02, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 1'000'000'000.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 110.0,
                                                       .barrier_type = kiyosi::BarrierType::up_and_out,
                                                       .rebate = 10.0,
                                                       .rebate_timing = kiyosi::RebateTiming::at_hit});

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE_FALSE(result.has_value());
    CHECK(result.error().category == kiyosi::ErrorCategory::invalid_result);
}

TEST_CASE("Zero barrier rebates ignore payment timing", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    for (const auto [rate, dividend] : {std::pair{-0.02, 0.0}, std::pair{-0.01, -0.02}}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(rate, dividend, 0.2), 100.0, start);
        for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
            for (const bool up : {true, false}) {
                CAPTURE(rate, dividend, type, up);
                auto terms = kiyosi::BarrierOptionTerms{
                    .option_type = type, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 120.0 : 80.0, .barrier_type = up ? kiyosi::BarrierType::up_and_out : kiyosi::BarrierType::down_and_out, .rebate = 0.0, .rebate_timing = kiyosi::RebateTiming::at_expiry};
                const auto expiry_price = kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context);
                REQUIRE(expiry_price);
                terms.rebate_timing = kiyosi::RebateTiming::at_hit;
                const auto hit_price = kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context);
                REQUIRE(hit_price);
                CHECK(*hit_price == *expiry_price);
            }
        }
    }
}

TEST_CASE("Barrier engines price prior touches from history instead of current spot")
{
    const auto effective = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry = day(2026, 1, 1);
    const auto parameters = kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    REQUIRE(parameters);
    const auto context_checked = kiyosi::make_pricing_context(
        *parameters, 100.0, valuation);
    REQUIRE(context_checked);
    const auto& context = *context_checked;
    auto terms = kiyosi::BarrierOptionTerms{.option_type = kiyosi::OptionType::call,
                                            .strike = 100.0,
                                            .effective_date = effective,
                                            .expiry_date = expiry,
                                            .barrier_level = 120.0,
                                            .barrier_type = kiyosi::BarrierType::up_and_out};
    const auto missing_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(missing_checked);
    const auto& missing = *missing_checked;
    CHECK_FALSE(missing.touch_state());
    const auto missing_result = kiyosi::AnalyticBarrierEngine{}.price(missing, context);
    REQUIRE_FALSE(missing_result);
    CHECK(missing_result.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const auto missing_fd = kiyosi::FiniteDifferenceBarrierEngine{}.price(missing, context);
    REQUIRE_FALSE(missing_fd.has_value());
    CHECK(missing_fd.error().category ==
          kiyosi::ErrorCategory::invalid_parameter);

    terms.touch_state = kiyosi::BarrierTouchState::untouched;
    const auto untouched_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(untouched_checked);
    const auto& untouched = *untouched_checked;
    terms.touch_state = kiyosi::BarrierTouchState::touched;
    const auto touched_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(touched_checked);
    const auto& touched = *touched_checked;
    CHECK(*kiyosi::AnalyticBarrierEngine{}.price(untouched, context) > 0.0);
    CHECK(*kiyosi::AnalyticBarrierEngine{}.price(touched, context) == 0.0);
    CHECK(*kiyosi::FiniteDifferenceBarrierEngine{}.price(touched, context) == 0.0);

    terms.barrier_type = kiyosi::BarrierType::up_and_in;
    const auto knocked_in_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(knocked_in_checked);
    const auto& knocked_in = *knocked_in_checked;
    const auto vanilla_checked = kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry);
    REQUIRE(vanilla_checked);
    const auto& vanilla = *vanilla_checked;
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(knocked_in, context),
               Catch::Matchers::WithinAbs(*kiyosi::AnalyticVanillaEngine{}.price(vanilla, context), 1e-12));

    terms.barrier_type = kiyosi::BarrierType::up_and_out;
    terms.rebate = 10.0;
    terms.rebate_timing = kiyosi::RebateTiming::at_hit;
    CHECK(*kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context) == 0.0);
    terms.rebate_timing = kiyosi::RebateTiming::at_expiry;
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context),
               Catch::Matchers::WithinAbs(10.0 * std::exp(-0.04 * 184.0 / 365.0), 1e-12));
}

TEST_CASE("Analytic scheduled barriers stop monitoring after their final observation")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, day(2025, 1, 3));
    const auto vanilla = *kiyosi::AnalyticVanillaEngine{}.price(
        *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry), context);
    auto terms = kiyosi::BarrierOptionTerms{.option_type = kiyosi::OptionType::call,
                                            .strike = 100.0,
                                            .effective_date = effective,
                                            .expiry_date = expiry,
                                            .barrier_level = 120.0,
                                            .barrier_type = kiyosi::BarrierType::up_and_out,
                                            .observation_mode = kiyosi::ObservationMode::scheduled,
                                            .observation_dates = {day(2025, 1, 2)},
                                            .touch_state = kiyosi::BarrierTouchState::untouched};
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context),
               Catch::Matchers::WithinAbs(vanilla, 1e-12));
    terms.barrier_type = kiyosi::BarrierType::up_and_in;
    terms.rebate = 10.0;
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(*kiyosi::make_barrier_option(terms), context),
               Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05 * 363.0 / 365.0), 1e-12));
}

TEST_CASE("Scheduled barriers settle exactly at and after their final fixing", "[audit-fixes]")
{
    const auto effective = day(2025, 1, 1);
    const auto fixing = day(2025, 1, 2);
    const auto expiry = day(2026, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto check = [&](const auto& engine) {
        for (const auto valuation : {effective, fixing, day(2025, 1, 3)}) {
            for (const auto kind : {kiyosi::BarrierType::up_and_in, kiyosi::BarrierType::up_and_out}) {
                for (const double rebate : {0.0, 10.0}) {
                    CAPTURE(valuation, kind, rebate);
                    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
                    const auto option = *kiyosi::make_barrier_option(
                        {.option_type = kiyosi::OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = kind, .rebate = rebate, .observation_mode = kiyosi::ObservationMode::scheduled, .observation_dates = {fixing}, .touch_state = kiyosi::BarrierTouchState::untouched});
                    const auto price = engine.price(option, context);
                    if constexpr (std::same_as<std::remove_cvref_t<decltype(engine)>, kiyosi::AnalyticBarrierEngine>) {
                        if (valuation == effective) {
                            REQUIRE_FALSE(price);
                            CHECK(price.error().category == kiyosi::ErrorCategory::unsupported_operation);
                            continue;
                        }
                    }
                    REQUIRE(price);
                    if (valuation == effective) {
                        CHECK(*price > 0.0);
                        continue;
                    }
                    const double expected = kind == kiyosi::BarrierType::up_and_in
                                                ? rebate * std::exp(-0.05 * *kiyosi::year_fraction(valuation, expiry))
                                                : *kiyosi::AnalyticVanillaEngine{}.price(
                                                      *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry), context);
                    CHECK(*price == expected);
                }
            }
        }
    };
    check(kiyosi::AnalyticBarrierEngine{});
    check(kiyosi::FiniteDifferenceBarrierEngine{});
}

TEST_CASE("Scheduled barrier fixing does not recur later on its observation date")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto fixing = day(2025, 1, 2);
    const auto time = kiyosi::start_of_day(fixing) + std::chrono::hours{12};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 125.0, time);
    const auto barrier = *kiyosi::make_barrier_option(
        {.option_type = kiyosi::OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = kiyosi::BarrierType::up_and_out, .observation_mode = kiyosi::ObservationMode::scheduled, .observation_dates = {fixing}, .touch_state = kiyosi::BarrierTouchState::untouched});
    const auto vanilla = *kiyosi::AnalyticVanillaEngine{}.price(
        *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry), context);
    CHECK_THAT(*kiyosi::AnalyticBarrierEngine{}.price(barrier, context),
               Catch::Matchers::WithinAbs(vanilla, 1e-12));
    CHECK_THAT(*kiyosi::FiniteDifferenceBarrierEngine{}.price(barrier, context),
               Catch::Matchers::WithinAbs(vanilla, 0.1));
}

} // namespace
