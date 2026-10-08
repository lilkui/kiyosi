#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <array>
#include <limits>
#include <type_traits>
#include <utility>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"
#include "pricing/detail/math.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;
using kiyosi::test::greek_value;
constexpr auto effective = day(2025, 1, 1);
constexpr auto valuation = day(2025, 7, 1);
constexpr auto expiry = day(2026, 1, 1);

PricingContext market(double spot = 100.0, Date date = valuation)
{
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
    return *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.3), spot, date);
}

struct RecordingPriceEngine {
    std::vector<PricingContext>* calls{};
    bool reject_bump = false;
    Result<double> price(const EuropeanOption&, const PricingContext& context) const
    {
        calls->push_back(context);
        if (reject_bump && context.spot_price() != 100.0)
            return std::unexpected(Error{ErrorCategory::invalid_schedule, "bump rejected"});
        return context.spot_price() * context.spot_price();
    }
};

template <typename Engine>
concept ImplicitGreeksRequest = requires(const Engine& engine, const EuropeanOption& option,
                                         const PricingContext& context) {
    engine.price_with_greeks(option, context);
};
static_assert(!ImplicitGreeksRequest<AnalyticVanillaEngine>);
template <typename Engine>
concept IntegerGreeksRequest = requires(const Engine& engine, const EuropeanOption& option,
                                        const PricingContext& context) {
    engine.price_with_greeks(option, context, 1);
};
static_assert(!IntegerGreeksRequest<AnalyticVanillaEngine>);
static_assert(std::is_convertible_v<bool, GreeksRequest>);
static_assert(std::is_constructible_v<GreeksRequest, const bool&>);
static_assert(!std::is_constructible_v<GreeksRequest, int>);
static_assert(!std::is_constructible_v<GreeksRequest, double>);
static_assert(!std::is_constructible_v<GreeksRequest, const char*>);
static_assert(!std::is_constructible_v<GreeksRequest, void*>);
static_assert(!std::is_constructible_v<GreeksRequest, std::nullptr_t>);
static_assert(std::is_constructible_v<GreeksRequest, std::initializer_list<Greek>>);
static_assert(std::is_same_v<decltype(AnalyticVanillaEngine{}.price(
                                 std::declval<const EuropeanOption&>(), std::declval<const PricingContext&>())),
                             Result<double>>);

TEST_CASE("Moved-from scheduled barriers reject pricing before accessing observations", "[pricing-api][audit-fixes]")
{
    const auto context = market(100.0, effective);
    const auto check = [&](auto option, const auto& engine) {
        const auto owner = std::move(option);
        REQUIRE(option.observation_dates().empty()); // NOLINT(bugprone-use-after-move,clang-analyzer-cplusplus.Move): this regression checks moved-from state.
        const auto price = engine.price(option, context);
        REQUIRE_FALSE(price);
        CHECK(price.error().category == ErrorCategory::invalid_schedule);
        const auto greeks = engine.price_with_greeks(option, context, {Greek::delta});
        REQUIRE_FALSE(greeks);
        CHECK(greeks.error().category == ErrorCategory::invalid_schedule);
        const auto quote = engine.price(owner, context);
        REQUIRE(quote);
        const auto implied = implied_volatility(engine, option, context, *quote);
        REQUIRE_FALSE(implied);
        CHECK(implied.error().category == ErrorCategory::invalid_schedule);
    };
    const auto barrier = *make_barrier_option(
        {.option_type = OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {expiry}});
    check(barrier, AnalyticBarrierEngine{});
    check(barrier, FiniteDifferenceBarrierEngine{});
    check(*make_cash_binary_barrier_option(
              {.option_type = OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {expiry}}, 1.0),
          AnalyticBinaryBarrierEngine{});
    check(*make_cash_one_touch_up(effective, expiry, 120.0, 1.0, SettlementTiming::at_expiry,
                                  ObservationMode::scheduled, {expiry}),
          AnalyticBinaryBarrierEngine{});
}

TEST_CASE("Exponential normal tails stay finite when separate factors overflow", "[audit-fixes]")
{
    for (const double value : {-9.0, -10.0, -11.0, -20.0, -35.0})
        CHECK(detail::exponential_normal_cdf(100.0, value) ==
              Catch::Approx(std::exp(100.0) * detail::normal_cdf(value)).epsilon(1e-12));
    // Mills' bounds independently constrain exp(x^2/2) Phi(-x) beyond the representable CDF range.
    for (const double value : {40.0, 100.0, 1000.0}) {
        const double tail = detail::exponential_normal_cdf(0.5 * value * value, -value);
        CHECK(tail > detail::inverse_sqrt_two_pi * value / (value * value + 1.0));
        CHECK(tail < detail::inverse_sqrt_two_pi / value);
    }
}

TEST_CASE("Analytic engines retain low-volatility prices and default implied volatility", "[audit-fixes]")
{
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, effective);
    const auto low = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.0001), 100.0, effective);
    const auto check = [&](const auto& option, const auto& engine, double expected) {
        const auto value = engine.price(option, low);
        REQUIRE(value);
        CHECK(*value == Catch::Approx(expected).margin(1e-9));
        const auto quote = engine.price(option, context);
        REQUIRE(quote);
        const auto implied = implied_volatility(engine, option, context, *quote);
        REQUIRE(implied);
        CHECK(*implied == Catch::Approx(0.2).margin(1e-6));
    };
    const BarrierOptionTerms terms{
        .option_type = OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out};
    const double vanilla = 100.0 * (std::exp(-0.02) - std::exp(-0.05));
    check(*make_barrier_option(terms), AnalyticBarrierEngine{}, vanilla);
    check(*make_cash_binary_barrier_option(
              {.option_type = OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out}, 1.0),
          AnalyticBinaryBarrierEngine{}, std::exp(-0.05));
    check(*make_american_option(OptionType::call, 100.0, effective, expiry), BjerksundStenslandVanillaEngine{}, vanilla);
    const auto negative_carry = *make_pricing_context(*make_bsm_parameters(0.02, 0.05, 0.0001), 80.0, effective);
    const auto out_of_money = BjerksundStenslandVanillaEngine{}.price(
        *make_american_option(OptionType::call, 100.0, effective, expiry), negative_carry);
    REQUIRE_FALSE(out_of_money);
    CHECK(out_of_money.error().category == ErrorCategory::unsupported_operation);
    const auto touch = AnalyticBinaryBarrierEngine{}.price(
        *make_cash_one_touch_up(effective, expiry, 120.0, 1.0, SettlementTiming::at_hit), low);
    REQUIRE(touch);
    CHECK(*touch == Catch::Approx(0.0).margin(1e-9));
}

TEST_CASE("Pricing API separates scalar selected and all-Greek outputs", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const auto context = market();
    const AnalyticVanillaEngine engine;
    const auto scalar = engine.price(option, context);
    const auto basic = engine.price_with_greeks(option, context, GreeksRequest{Greek::delta, Greek::gamma});
    const auto full = engine.price_with_greeks(option, context, GreeksRequest{true});
    REQUIRE(scalar);
    REQUIRE(basic);
    REQUIRE(full);
    CHECK(basic->price() == *scalar);
    CHECK(full->price() == *scalar);
    CHECK(greek_value(*basic, Greek::delta) == greek_value(*full, Greek::delta));
    CHECK(greek_value(*basic, Greek::gamma) == greek_value(*full, Greek::gamma));
    for (std::size_t i = 0; i < greek_count; ++i) {
        const auto measure = static_cast<Greek>(i);
        CAPTURE(i);
        CHECK(full->has(measure));
        CHECK(basic->has(measure) == (measure == Greek::delta || measure == Greek::gamma));
    }
}

TEST_CASE("Selected Greek completion uses only missing spot differences", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const auto context = market();
    std::vector<PricingContext> calls;
    const RecordingPriceEngine engine{&calls};
    const auto native = *make_pricing_result(10000.0);
    const auto basic = detail::complete_greeks(engine, option, context, GreeksRequest{Greek::delta, Greek::gamma}, {}, native);
    REQUIRE(basic);
    REQUIRE(calls.size() == 2);
    CHECK(calls[0].spot_price() == 100.0 + NumericalShiftSettings{}.spot_shift);
    CHECK(calls[1].spot_price() == 100.0 - NumericalShiftSettings{}.spot_shift);
    for (const auto& call : calls) {
        CHECK(call.valuation_time() == context.valuation_time());
        CHECK(call.model_parameters().volatility() == 0.3);
        CHECK(call.model_parameters().risk_free_rate() == 0.04);
    }
    CHECK(greek_value(*basic, Greek::delta) == Catch::Approx(200.0));
    CHECK(greek_value(*basic, Greek::gamma) == Catch::Approx(2.0).margin(1e-5));
    calls.clear();
    const auto supplied = *make_pricing_result(10000.0, {{Greek::delta, 17.0},
                                                         {Greek::gamma, 0.0}});
    const auto preserved = detail::complete_greeks(engine, option, context, GreeksRequest{Greek::delta, Greek::gamma}, {}, supplied);
    REQUIRE(preserved);
    CHECK(calls.empty());
    CHECK(greek_value(*preserved, Greek::delta) == 17.0);
    CHECK(greek_value(*preserved, Greek::gamma) == 0.0);
    const auto full = detail::complete_greeks(engine, option, context, GreeksRequest{true}, {}, supplied);
    REQUIRE(full);
    CHECK_FALSE(calls.empty());
    CHECK(greek_value(*full, Greek::delta) == 17.0);
    CHECK(greek_value(*full, Greek::gamma) == 0.0);
    CHECK(greek_value(*full, Greek::vega) == 0.0);
    CHECK(greek_value(*full, Greek::rho) == 0.0);
}

TEST_CASE("Joint pricing calculates only requested Greeks", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    std::vector<PricingContext> calls;
    const RecordingPriceEngine engine{&calls};
    const auto gamma = detail::price_with_greeks(
        engine, option, market(), GreeksRequest{Greek::gamma}, {},
        [&](const auto&) -> Result<PricingResult> {
            return make_pricing_result(10000.0);
        });
    REQUIRE(gamma);
    CHECK(calls.size() == 2);
    CHECK(std::isfinite(gamma->price()));
    CHECK(gamma->has(Greek::gamma));
    CHECK_FALSE(gamma->has(Greek::delta));
    CHECK_FALSE(gamma->has(Greek::vega));

    calls.clear();
    const auto rho = detail::price_with_greeks(
        engine, option, market(), GreeksRequest{Greek::rho}, {},
        [&](const auto&) -> Result<PricingResult> {
            return make_pricing_result(10000.0);
        });
    REQUIRE(rho);
    REQUIRE(calls.size() == 2);
    for (const auto& context : calls)
        CHECK(context.spot_price() == 100.0);
    CHECK(greek_value(*rho, Greek::rho) == 0.0);
    CHECK_FALSE(rho->has(Greek::gamma));

    const auto analytic = AnalyticVanillaEngine{}.price_with_greeks(
        option, market(), {Greek::vega, Greek::rho, Greek::vega});
    REQUIRE(analytic);
    CHECK(analytic->has(Greek::vega));
    CHECK(analytic->has(Greek::rho));
    CHECK_FALSE(analytic->has(Greek::delta));
    CHECK_FALSE(analytic->has(Greek::gamma));
    REQUIRE(AnalyticVanillaEngine{}.price_with_greeks(option, market(), true));
}

TEST_CASE("Unrepresentable shifts leave numerical sensitivities unavailable", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const auto normal = calculate_numerical_greeks(AnalyticVanillaEngine{}, option, market());
    REQUIRE(normal);
    CHECK(greek_value(*normal, Greek::vega) > 0.0);
    CHECK(greek_value(*normal, Greek::rho) > 0.0);
    const auto tiny = calculate_numerical_greeks(AnalyticVanillaEngine{}, option, market(),
                                                 {.volatility_shift = 1e-20, .rate_shift = 1e-20});
    REQUIRE(tiny);
    for (const auto greek : {Greek::vega, Greek::vanna, Greek::zomma, Greek::rho})
        CHECK_FALSE(tiny->has(greek));
    CHECK(tiny->has(Greek::delta));
}

TEST_CASE("Joint pricing validates discriminators and every shift", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const AnalyticVanillaEngine engine;
    const auto invalid_request = engine.price_with_greeks(option, market(), {static_cast<Greek>(99)});
    REQUIRE_FALSE(invalid_request);
    CHECK(invalid_request.error().category == ErrorCategory::invalid_parameter);
    for (const auto request : {GreeksRequest{}, GreeksRequest{false}}) {
        const auto invalid = engine.price_with_greeks(option, market(), request);
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
    for (const auto shifts : {NumericalShiftSettings{0.0},
                              NumericalShiftSettings{.volatility_shift = -0.1},
                              NumericalShiftSettings{.rate_shift = std::numeric_limits<double>::infinity()},
                              NumericalShiftSettings{.time_shift_days = 0}}) {
        const auto result = engine.price_with_greeks(option, market(), GreeksRequest{Greek::delta, Greek::gamma}, shifts);
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::invalid_parameter);
    }
}

TEST_CASE("Joint completion keeps unavailable stencils and propagates feasible failures", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    std::vector<PricingContext> calls;
    const auto native = *make_pricing_result(10000.0);
    const auto unavailable = detail::complete_greeks(RecordingPriceEngine{&calls}, option,
                                                     market(), GreeksRequest{Greek::delta, Greek::gamma}, NumericalShiftSettings{.spot_shift = 100.0}, native);
    REQUIRE(unavailable);
    CHECK(unavailable->price() == 10000.0);
    CHECK_FALSE(unavailable->has(Greek::delta));
    CHECK_FALSE(unavailable->has(Greek::gamma));
    CHECK(calls.empty());
    const auto rejected = detail::complete_greeks(RecordingPriceEngine{&calls, true}, option,
                                                  market(), GreeksRequest{Greek::delta, Greek::gamma}, {}, native);
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_schedule);
    CHECK(rejected.error().message == "bump rejected");
    CHECK(calls.size() == 1);
}

TEST_CASE("Expiry suppresses all requested Greeks", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const auto check = [&](const auto& engine) {
        for (const auto level : {GreeksRequest{Greek::delta, Greek::gamma}, GreeksRequest{true}}) {
            const auto result = engine.price_with_greeks(option, market(110.0, expiry), level);
            REQUIRE(result);
            CHECK(result->price() == 10.0);
            for (std::size_t i = 0; i < greek_count; ++i)
                CHECK_FALSE(result->has(static_cast<Greek>(i)));
        }
    };
    check(AnalyticVanillaEngine{});
    check(QuadratureVanillaEngine{});
    check(CoxRossRubinsteinVanillaEngine{8});
    check(FiniteDifferenceVanillaEngine{20, 20});
    check(MonteCarloVanillaEngine{32, 2, 7});

    std::vector<PricingContext> calls;
    const RecordingPriceEngine engine{&calls};
    const auto native = *make_pricing_result(10.0, {{Greek::delta, 1.0}});
    const auto completed = detail::complete_greeks(
        engine, option, market(110.0, expiry), true, {}, native);
    REQUIRE(completed);
    CHECK(completed->price() == 10.0);
    CHECK_FALSE(completed->has(Greek::delta));
    for (const bool native_complete : {false, true}) {
        const auto result = detail::price_with_greeks(
            engine, option, market(110.0, expiry), true, {},
            [&](const auto&) -> Result<PricingResult> { return native; }, native_complete);
        REQUIRE(result);
        CHECK(result->price() == 10.0);
        CHECK_FALSE(result->has(Greek::delta));
    }
    CHECK(calls.empty());
}

TEST_CASE("Monitored barrier equality leaves all Greeks unavailable", "[pricing-api]")
{
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 100.0, .barrier_type = BarrierType::down_and_out, .rebate = 2.0, .touch_state = BarrierTouchState::untouched});
    const AnalyticBarrierEngine engine;
    const auto scalar = engine.price(option, market());
    REQUIRE(scalar);
    for (const auto level : {GreeksRequest{Greek::delta, Greek::gamma}, GreeksRequest{true}}) {
        const auto result = engine.price_with_greeks(option, market(), level);
        REQUIRE(result);
        CHECK(result->price() == *scalar);
        for (std::size_t i = 0; i < greek_count; ++i)
            CHECK_FALSE(result->has(static_cast<Greek>(i)));
    }
}

TEST_CASE("Extreme time shift is bounded before timestamp arithmetic", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    std::vector<PricingContext> calls;
    const auto result = calculate_numerical_greeks(RecordingPriceEngine{&calls}, option,
                                                   market(), NumericalShiftSettings{.time_shift_days = std::numeric_limits<int>::max()});
    REQUIRE(result);
    CHECK(greek_value(*result, Greek::theta) == 0.0);
    for (const auto& call : calls) {
        CHECK(call.valuation_time() >= start_of_day(effective));
        CHECK(call.valuation_time() <= start_of_day(expiry));
    }
}

TEST_CASE("Seeded Monte Carlo joint pricing is reproducible and matches scalar pricing", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const MonteCarloVanillaEngine engine{256, 4, 73};
    const auto scalar = engine.price(option, market());
    REQUIRE(scalar);
    for (const auto level : {GreeksRequest{Greek::delta, Greek::gamma}, GreeksRequest{true}}) {
        const auto first = engine.price_with_greeks(option, market(), level);
        const auto second = engine.price_with_greeks(option, market(), level);
        REQUIRE(first);
        REQUIRE(second);
        CHECK(first->price() == *scalar);
        for (std::size_t i = 0; i < greek_count; ++i)
            CHECK(first->values_view()[i] == second->values_view()[i]);
        CHECK(greek_value(*first, Greek::delta) >= 0.0);
        CHECK(greek_value(*first, Greek::delta) <= 1.2);
    }
    CHECK(engine.settings().seed == 73);
}
TEST_CASE("Scalar and basic analytic pricing avoid overflowing higher Greeks", "[pricing-api]")
{
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 1e-160,
                                                      effective, expiry);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto context = market(1e-160);
    const auto scalar = engine.price(option, context);
    const auto basic = engine.price_with_greeks(option, context, kiyosi::GreeksRequest{kiyosi::Greek::delta, kiyosi::Greek::gamma});
    const auto full = engine.price_with_greeks(option, context, kiyosi::GreeksRequest{true});
    REQUIRE(scalar);
    REQUIRE(basic);
    CHECK(*scalar > 0.0);
    CHECK(basic->price() == *scalar);
    CHECK(greek_value(*basic, kiyosi::Greek::gamma) > 1e150);
    const auto rho_only = engine.price_with_greeks(option, context, {kiyosi::Greek::rho});
    REQUIRE(rho_only);
    CHECK(rho_only->has(kiyosi::Greek::rho));
    CHECK_FALSE(rho_only->has(kiyosi::Greek::gamma));
    REQUIRE_FALSE(full);
    CHECK(full.error().category == kiyosi::ErrorCategory::invalid_result);
}
struct SeedRecordingSettings {
    std::optional<unsigned> seed;
    std::vector<std::optional<unsigned>>* calls;
};
struct SeedRecordingEngine {
    SeedRecordingSettings configuration;
    const SeedRecordingSettings& settings() const { return configuration; }
    kiyosi::Result<double> price(const kiyosi::EuropeanOption&,
                                 const kiyosi::PricingContext& context) const
    {
        configuration.calls->push_back(configuration.seed);
        return context.spot_price() * context.spot_price();
    }
};

TEST_CASE("Unseeded joint pricing selects one seed without changing the engine", "[pricing-api]")
{
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry);
    std::vector<std::optional<unsigned>> calls;
    const SeedRecordingEngine engine{{std::nullopt, &calls}};
    const auto result = kiyosi::detail::price_with_greeks(engine, option, market(),
                                                          kiyosi::GreeksRequest{kiyosi::Greek::delta, kiyosi::Greek::gamma}, {}, [&](const auto& seeded) -> kiyosi::Result<kiyosi::PricingResult> {
                                                              const auto value = seeded.price(option, market());
                                                              if (!value) return std::unexpected(value.error());
                                                              return kiyosi::make_pricing_result(*value);
                                                          });
    REQUIRE(result);
    REQUIRE(calls.size() == 3);
    REQUIRE(calls.front().has_value());
    for (const auto seed : calls)
        CHECK(seed == calls.front());
    CHECK_FALSE(engine.settings().seed.has_value());
    CHECK(greek_value(*result, kiyosi::Greek::delta) == Catch::Approx(200.0));
}

struct SolverSeedRecordingEngine {
    SeedRecordingSettings configuration;
    const SeedRecordingSettings& settings() const { return configuration; }
    kiyosi::Result<double> price(const kiyosi::EuropeanOption&, const kiyosi::PricingContext& context) const
    {
        configuration.calls->push_back(configuration.seed);
        return context.model_parameters().volatility();
    }
    kiyosi::Result<double> price(const kiyosi::PhoenixOption& option, const kiyosi::PricingContext&) const
    {
        configuration.calls->push_back(configuration.seed);
        return option.coupon_rate();
    }
};

TEST_CASE("Implied solvers control price and parameter tolerances separately", "[pricing-api]")
{
    const auto option = *make_european_option(OptionType::call, 100.0, effective, expiry);
    const auto phoenix = *make_phoenix_option({.coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0}, .coupon_barrier_levels = {90.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {expiry}, .effective_date = effective, .expiry_date = expiry});
    std::vector<std::optional<unsigned>> calls;
    const SolverSeedRecordingEngine engine{{1, &calls}};
    for (const auto [price_tolerance, parameter_tolerance] :
         {std::pair{1e-12, 1e-12}, std::pair{0.15, 1e-12}, std::pair{1e-12, 1.0},
          std::pair{0.0, 1e-12}, std::pair{1e-12, 0.0}}) {
        const auto vol = kiyosi::implied_volatility(engine, option, market(), 0.37,
                                                    {0.1, 0.9, price_tolerance, parameter_tolerance, 1});
        const auto coupon = kiyosi::implied_coupon(engine, phoenix, market(), 0.37,
                                                   {0.1, 0.9, price_tolerance, parameter_tolerance, 1});
        for (const auto& result : {vol, coupon}) {
            if (price_tolerance == 0.0 || parameter_tolerance == 0.0) {
                REQUIRE_FALSE(result);
                CHECK(result.error().category == ErrorCategory::invalid_parameter);
            } else if (price_tolerance == parameter_tolerance) {
                REQUIRE_FALSE(result);
                CHECK(result.error().category == ErrorCategory::solver_non_convergence);
            } else {
                REQUIRE(result);
                CHECK(*result == 0.5);
            }
        }
    }
}

TEST_CASE("Implied solvers keep one seed for every trial without changing the engine", "[pricing-api]")
{
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective, expiry);
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0}, .coupon_barrier_levels = {90.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {expiry}, .effective_date = effective, .expiry_date = expiry});
    std::vector<std::optional<unsigned>> calls;
    const SolverSeedRecordingEngine engine{{std::nullopt, &calls}};
    const auto check_calls = [&] {
        REQUIRE(calls.size() > 2);
        REQUIRE(calls.front().has_value());
        for (const auto seed : calls)
            CHECK(seed == calls.front());
        calls.clear();
    };

    const auto volatility = kiyosi::implied_volatility(engine, option, market(), 0.3);
    REQUIRE(volatility);
    CHECK(*volatility == Catch::Approx(0.3).margin(1e-8));
    check_calls();

    const auto coupon = kiyosi::implied_coupon(engine, phoenix, market(), 0.125);
    REQUIRE(coupon);
    CHECK(*coupon == Catch::Approx(0.125).margin(1e-8));
    check_calls();
    CHECK_FALSE(engine.settings().seed.has_value());

    const SolverSeedRecordingEngine seeded{{73, &calls}};
    REQUIRE(kiyosi::implied_volatility(seeded, option, market(), 0.3));
    REQUIRE(calls.size() > 2);
    for (const auto seed : calls)
        CHECK(seed == 73);
}
TEST_CASE("Implied accumulator volatility rejects immediate knock-out settlements", "[pricing-api]")
{
    const auto option = *make_accumulator(
        {.strike = 100.0, .knock_out_level = 120.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 5.0, .effective_date = effective, .expiry_date = expiry});
    const auto check = [&](const auto& engine) {
        for (const double spot : {120.0, 130.0}) {
            const auto context = market(spot);
            const auto quote = engine.price(option, context);
            REQUIRE(quote);
            CHECK(*quote == 5.0 * (spot - 100.0));
            const auto implied = implied_volatility(engine, option, context, *quote);
            REQUIRE_FALSE(implied);
            CHECK(implied.error().category == ErrorCategory::unsupported_operation);
        }
        for (const auto time : {start_of_day(valuation) + std::chrono::hours{12},
                                start_of_day(day(2025, 7, 5))}) {
            const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.3), 130.0, time);
            const auto quote = engine.price(option, context);
            REQUIRE(quote);
            const auto implied = implied_volatility(engine, option, context, *quote,
                                                    {.lower_bound = 0.3, .upper_bound = 0.4});
            REQUIRE(implied);
            CHECK(*implied == 0.3);
        }
    };
    check(MonteCarloAccumulatorEngine{{64, 7}});
    check(FiniteDifferenceAccumulatorEngine{{.asset_step_count = 40, .time_step_count = 40}});
}

TEST_CASE("Event thresholds suppress spot Greeks but retain rate and volatility sensitivities", "[pricing-api]")
{
    const auto accumulator = *kiyosi::make_accumulator({.strike = 90.0, .knock_out_level = 100.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 3.0, .effective_date = effective, .expiry_date = expiry});
    const auto snowball = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.1, 0.1},
                                                                .maturity_coupon_rate = 0.05,
                                                                .knock_out_levels = {100.0, 100.0},
                                                                .observation_dates = {valuation, expiry},
                                                                .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                .principal_ratio = 1.0,
                                                                .effective_date = effective,
                                                                .expiry_date = expiry});
    const auto check = [&](const auto& engine, const auto& option) {
        const auto result = engine.price_with_greeks(option, market(), kiyosi::GreeksRequest{true});
        REQUIRE(result);
        CHECK(result->price() == *engine.price(option, market()));
        CHECK(greek_value(*result, kiyosi::Greek::vega) == 0.0);
        CHECK(greek_value(*result, kiyosi::Greek::rho) == 0.0);
        for (const auto measure : {kiyosi::Greek::delta, kiyosi::Greek::gamma,
                                   kiyosi::Greek::speed, kiyosi::Greek::theta, kiyosi::Greek::charm,
                                   kiyosi::Greek::color, kiyosi::Greek::vanna, kiyosi::Greek::zomma})
            CHECK_FALSE(result->has(measure));
        const auto noon = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.04, 0.01, 0.3),
                                                        100.0, kiyosi::start_of_day(valuation) + std::chrono::hours{12});
        const auto after = engine.price_with_greeks(option, noon, kiyosi::GreeksRequest{kiyosi::Greek::delta, kiyosi::Greek::gamma});
        REQUIRE(after);
        REQUIRE(after->has(kiyosi::Greek::delta));
        REQUIRE(after->has(kiyosi::Greek::gamma));
        CHECK(std::isfinite(greek_value(*after, kiyosi::Greek::delta)));
        CHECK(std::isfinite(greek_value(*after, kiyosi::Greek::gamma)));
    };
    check(kiyosi::MonteCarloAccumulatorEngine{{64, 7}}, accumulator);
    check(kiyosi::MonteCarloBinarySnowballEngine{{64, 7}}, snowball);
}

TEST_CASE("Spot Greeks omit bumps crossing current event thresholds", "[pricing-api]")
{
    const auto check = [&](const auto& engine, const auto& option, double spot) {
        const auto context = market(spot);
        const auto result = engine.price_with_greeks(option, context, GreeksRequest{true});
        REQUIRE(result);
        CHECK(result->price() == *engine.price(option, context));
        CHECK(result->has(Greek::vega));
        CHECK(result->has(Greek::rho));
        for (const auto measure : {Greek::delta, Greek::gamma, Greek::speed,
                                   Greek::vanna, Greek::zomma, Greek::charm, Greek::color})
            CHECK_FALSE(result->has(measure));
        const auto narrow = calculate_numerical_greeks(
            engine, option, context, NumericalShiftSettings{.spot_shift = 0.0001});
        REQUIRE(narrow);
        CHECK(narrow->has(Greek::delta));
        CHECK(narrow->has(Greek::gamma));
        CHECK(narrow->has(Greek::speed));
    };
    for (const auto mode : {ObservationMode::continuous, ObservationMode::scheduled}) {
        CAPTURE(mode);
        const auto option = *make_barrier_option(
            {.option_type = OptionType::call, .strike = 100.0, .effective_date = effective, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .rebate = 10.0, .observation_mode = mode, .observation_dates = mode == ObservationMode::scheduled ? std::vector<Date>{valuation, expiry} : std::vector<Date>{}, .touch_state = BarrierTouchState::untouched});
        check(AnalyticBarrierEngine{}, option, 119.999);
        const auto wider = calculate_numerical_greeks(AnalyticBarrierEngine{}, option, market(119.985));
        REQUIRE(wider);
        CHECK(wider->has(Greek::delta));
        CHECK(wider->has(Greek::gamma));
        CHECK_FALSE(wider->has(Greek::speed));
    }
    const auto accumulator = *make_accumulator(
        {.strike = 90.0, .knock_out_level = 100.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 3.0, .effective_date = effective, .expiry_date = expiry});
    check(MonteCarloAccumulatorEngine{{32, 7}}, accumulator, 99.999);
    check(MonteCarloAccumulatorEngine{{32, 7}}, accumulator, 90.001);
    const auto note = *make_phoenix_option(
        {.coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 70.0, .knock_out_levels = {120.0, 120.0}, .coupon_barrier_levels = {90.0, 90.0}, .upper_strike = 100.0, .lower_strike = 0.0, .observation_dates = {valuation, expiry}, .barrier_state = AutocallableBarrierState::none, .effective_date = effective, .expiry_date = expiry});
    for (const double spot : {119.999, 89.999, 70.001})
        check(MonteCarloPhoenixEngine{{32, 7}}, note, spot);
}

TEST_CASE("Time Greeks omit stencils requiring unavailable barrier history", "[pricing-api]")
{
    const auto start = day(2025, 1, 1);
    const auto option = *kiyosi::make_barrier_option(
        {.option_type = kiyosi::OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = day(2026, 1, 1), .barrier_level = 120.0, .barrier_type = kiyosi::BarrierType::up_and_out});
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto result = kiyosi::AnalyticBarrierEngine{}.price_with_greeks(
        option, context, kiyosi::GreeksRequest{true});
    REQUIRE(result);
    CHECK(std::isfinite(result->price()));
    CHECK(result->has(kiyosi::Greek::delta));
    CHECK_FALSE(result->has(kiyosi::Greek::theta));
    CHECK_FALSE(result->has(kiyosi::Greek::charm));
    CHECK_FALSE(result->has(kiyosi::Greek::color));
}

TEST_CASE("Implied Phoenix coupon requires a payable coupon", "[pricing-api][audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto check = [&](const auto& engine) {
        for (const bool at_expiry : {true, false}) {
            for (const double spot : {80.0, 90.0, 120.0}) {
                CAPTURE(at_expiry, spot);
                const auto date = at_expiry ? end : day(2025, 7, 1);
                const double barrier = at_expiry ? 90.0 : 130.0;
                const auto note = *kiyosi::make_phoenix_option(
                    {.coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 70.0, .knock_out_levels = {120.0, 120.0}, .coupon_barrier_levels = {barrier, 90.0}, .upper_strike = 100.0, .lower_strike = 0.0, .observation_dates = {day(2025, 7, 1), end}, .barrier_state = kiyosi::AutocallableBarrierState::none, .effective_date = start, .expiry_date = end});
                const auto context = *kiyosi::make_pricing_context(parameters, spot, date);
                const auto quote = engine.price(note, context);
                REQUIRE(quote);
                const auto implied = kiyosi::implied_coupon(engine, note, context, *quote);
                if ((at_expiry && spot < barrier) || (!at_expiry && spot >= 120.0)) {
                    REQUIRE_FALSE(implied);
                    CHECK(implied.error().category == kiyosi::ErrorCategory::unsupported_operation);
                } else {
                    REQUIRE(implied);
                    CHECK(*implied == Catch::Approx(0.1).margin(1e-7));
                }
            }
        }
    };
    check(kiyosi::MonteCarloPhoenixEngine{{64, 1}});
    check(kiyosi::FiniteDifferencePhoenixEngine{{.asset_step_count = 40, .time_step_count = 40, .asset_upper_boundary = 400.0}});
}

TEST_CASE("Implied Snowball coupon respects terminal knock-in and quote convention", "[pricing-api][audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto note = *kiyosi::make_snowball_option(
        {.knock_out_coupon_rates = {0.1}, .maturity_coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 70.0, .knock_out_levels = {120.0}, .upper_strike = 100.0, .lower_strike = 0.0, .observation_dates = {end}, .barrier_state = kiyosi::AutocallableBarrierState::none, .effective_date = start, .expiry_date = end});
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto check = [&](const auto& engine) {
        for (const double spot : {60.0, 80.0, 120.0}) {
            CAPTURE(spot);
            const auto context = *kiyosi::make_pricing_context(parameters, spot, end);
            const auto quote = engine.price(note, context);
            REQUIRE(quote);
            for (const auto convention : {kiyosi::CouponQuoteConvention::shift_maturity_coupon,
                                          kiyosi::CouponQuoteConvention::preserve_maturity_coupon}) {
                const auto implied = kiyosi::implied_coupon(engine, note, context, *quote, convention);
                if (spot < 70.0 || (spot < 120.0 && convention == kiyosi::CouponQuoteConvention::preserve_maturity_coupon)) {
                    REQUIRE_FALSE(implied);
                    CHECK(implied.error().category == kiyosi::ErrorCategory::unsupported_operation);
                } else {
                    REQUIRE(implied);
                    CHECK(*implied == Catch::Approx(0.1).margin(1e-7));
                }
            }
        }
    };
    check(kiyosi::MonteCarloSnowballEngine{{64, 1}});
    check(kiyosi::FiniteDifferenceSnowballEngine{{40, 40}});
}

TEST_CASE("Implied Snowball coupon rejects moved-from schedules before replacement", "[pricing-api][audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto context = market(150.0, end);
    const auto check = [&](auto note, const auto& engine) {
        const auto owner = std::move(note);
        REQUIRE(note.observation_dates().empty()); // NOLINT(bugprone-use-after-move,clang-analyzer-cplusplus.Move): this regression checks moved-from state.
        REQUIRE(note.knock_out_coupon_rates().empty());
        const auto rejected_price = engine.price(note, context);
        REQUIRE_FALSE(rejected_price);
        CHECK(rejected_price.error().category == ErrorCategory::invalid_schedule);
        const auto quote = engine.price(owner, context);
        REQUIRE(quote);
        CHECK(*quote == Catch::Approx(1.1).margin(1e-12));
        for (const auto convention : {CouponQuoteConvention::preserve_maturity_coupon,
                                      CouponQuoteConvention::shift_maturity_coupon}) {
            const auto rejected = implied_coupon(engine, note, context, *quote, convention);
            REQUIRE_FALSE(rejected);
            CHECK(rejected.error().category == ErrorCategory::invalid_schedule);
            const auto solved = implied_coupon(engine, owner, context, *quote, convention);
            REQUIRE(solved);
            CHECK(*solved == Catch::Approx(0.1).margin(1e-7));
        }
    };
    const auto snowball = *make_snowball_option(
        {.knock_out_coupon_rates = {0.1}, .maturity_coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 70.0, .knock_out_levels = {120.0}, .upper_strike = 100.0, .lower_strike = 0.0, .observation_dates = {end}, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end});
    const auto binary = *make_binary_snowball_option(
        {.knock_out_coupon_rates = {0.1}, .maturity_coupon_rate = 0.1, .knock_out_levels = {120.0}, .observation_dates = {end}, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end});
    const auto ternary = *make_ternary_snowball_option(
        {.knock_out_coupon_rates = {0.1}, .maturity_coupon_rate = 0.1, .minimum_coupon_rate = 0.0, .knock_in_level = 70.0, .knock_out_levels = {120.0}, .observation_dates = {end}, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end});
    check(snowball, MonteCarloSnowballEngine{32});
    check(snowball, FiniteDifferenceSnowballEngine{});
    check(binary, MonteCarloBinarySnowballEngine{32});
    check(binary, FiniteDifferenceBinarySnowballEngine{});
    check(ternary, MonteCarloTernarySnowballEngine{32});
    check(ternary, FiniteDifferenceTernarySnowballEngine{});
}

TEST_CASE("Implied volatility rejects fixed remaining cashflows", "[pricing-api]")
{
    const auto check = [&](const auto& engine) {
        for (const auto date : {effective, effective + std::chrono::days{1}}) {
            const auto context = market(100.0, date);
            const auto note = *make_binary_snowball_option(
                {.knock_out_coupon_rates = {0.3, 0.1}, .maturity_coupon_rate = 0.1, .knock_out_levels = {120.0, 120.0}, .observation_dates = {effective, expiry}, .barrier_state = AutocallableBarrierState::none, .effective_date = effective, .expiry_date = expiry});
            const double quote = 1.1 * std::exp(-0.04 * *year_fraction(date, expiry));
            const auto result = implied_volatility(engine, note, context, quote);
            REQUIRE_FALSE(result);
            CHECK(result.error().category == ErrorCategory::unsupported_operation);
        }
        const auto context = market(100.0, effective);
        const auto note = *make_binary_snowball_option(
            {.knock_out_coupon_rates = {0.2}, .maturity_coupon_rate = 0.1, .knock_out_levels = {120.0}, .observation_dates = {expiry}, .effective_date = effective, .expiry_date = expiry});
        const auto low = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.05), 100.0, effective);
        const auto quote = engine.price(note, low);
        REQUIRE(quote);
        const auto result = implied_volatility(engine, note, context, *quote, {.lower_bound = 0.05, .upper_bound = 0.4});
        REQUIRE(result);
        CHECK(*result == 0.05);
        const auto early_note = *make_binary_snowball_option(
            {.knock_out_coupon_rates = {0.0, 0.0}, .maturity_coupon_rate = 0.0, .knock_out_levels = {120.0, 120.0}, .observation_dates = {valuation, expiry}, .effective_date = effective, .expiry_date = expiry});
        for (const double rate : {0.0, 0.04}) {
            const auto low_context = *make_pricing_context(*make_bsm_parameters(rate, 0.01, 0.05), 100.0, effective);
            const auto early_quote = engine.price(early_note, low_context);
            REQUIRE(early_quote);
            const auto early_result = implied_volatility(engine, early_note, low_context, *early_quote,
                                                         {.lower_bound = 0.05, .upper_bound = 0.4});
            if (rate == 0.0) {
                REQUIRE_FALSE(early_result);
                CHECK(early_result.error().category == ErrorCategory::unsupported_operation);
            } else {
                REQUIRE(early_result);
                CHECK(*early_result == 0.05);
            }
        }
    };
    check(MonteCarloBinarySnowballEngine{{64, 73}});
    check(FiniteDifferenceBinarySnowballEngine{});

    const auto accumulator = *make_accumulator(
        {.strike = 100.0, .knock_out_level = 120.0, .daily_quantity = 0.0, .acceleration_factor = 1.0, .accumulated_quantity = 0.0, .effective_date = effective, .expiry_date = expiry});
    const auto context = market(100.0, effective);
    const auto check_accumulator = [&](const auto& engine) {
        const auto result = implied_volatility(engine, accumulator, context, 0.0);
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::unsupported_operation);
    };
    check_accumulator(MonteCarloAccumulatorEngine{{64, 73}});
    check_accumulator(FiniteDifferenceAccumulatorEngine{});
}

TEST_CASE("Implied ternary volatility requires remaining cashflow exposure", "[pricing-api][audit-fixes]")
{
    struct Scenario {
        double rate;
        double maturity_coupon;
        double minimum_coupon;
        double knock_out_coupon;
        AutocallableBarrierState state;
        Date date;
        bool exposed;
    };
    const auto check = [&](const auto& engine) {
        for (const Scenario scenario : {
                 Scenario{0.0, 0.0, 0.0, 0.0, AutocallableBarrierState::none, effective, false},
                 {0.0, 0.2, 0.0, 0.0, AutocallableBarrierState::none, effective, true},
                 {0.0, 0.2, 0.0, 0.0, AutocallableBarrierState::knocked_in, valuation, false},
                 {0.04, 0.0, 0.0, 0.0, AutocallableBarrierState::none, effective, true},
                 {0.0, 0.1, 0.1, 0.0, AutocallableBarrierState::none, effective, true},
                 {0.0, 0.2, 0.1, 0.2, AutocallableBarrierState::knocked_in, valuation, true}}) {
            CAPTURE(scenario.rate, scenario.maturity_coupon, scenario.minimum_coupon,
                    scenario.knock_out_coupon, scenario.state, scenario.date);
            const auto note = *make_ternary_snowball_option(
                {.knock_out_coupon_rates = {scenario.knock_out_coupon, scenario.knock_out_coupon},
                 .maturity_coupon_rate = scenario.maturity_coupon,
                 .minimum_coupon_rate = scenario.minimum_coupon,
                 .knock_in_level = 70.0,
                 .knock_out_levels = {120.0, 120.0},
                 .observation_dates = {valuation, expiry},
                 .barrier_state = scenario.state,
                 .effective_date = effective,
                 .expiry_date = expiry});
            const auto context = *make_pricing_context(
                *make_bsm_parameters(scenario.rate, 0.0, 0.05), 100.0, scenario.date);
            const auto quote = engine.price(note, context);
            REQUIRE(quote);
            const auto result = implied_volatility(engine, note, context, *quote,
                                                   {.lower_bound = 0.05, .upper_bound = 0.4});
            if (scenario.exposed) {
                REQUIRE(result);
                CHECK(*result == 0.05);
            } else {
                CHECK(*quote == Catch::Approx(1.0));
                REQUIRE_FALSE(result);
                CHECK(result.error().category == ErrorCategory::unsupported_operation);
            }
        }
    };
    check(MonteCarloTernarySnowballEngine{{64, 73}});
    check(FiniteDifferenceTernarySnowballEngine{});
}

TEST_CASE("Finite differences settle determined cashflows without a spatial grid", "[pricing-api][audit-fixes]")
{
    const auto check = [&](const auto& option, const auto& monte_carlo, const auto& finite_difference) {
        using Engine = std::remove_cvref_t<decltype(finite_difference)>;
        for (const Date date : {valuation, expiry}) {
            const auto context = market(1e308, date);
            const auto expected = monte_carlo.price(option, context);
            REQUIRE(expected);
            for (const auto& engine : {finite_difference, Engine{{.asset_upper_boundary = 1.0}}}) {
                const auto actual = engine.price(option, context);
                REQUIRE(actual);
                CHECK(*actual == *expected);
            }
            const auto invalid = Engine{{.asset_step_count = 2}}.price(option, context);
            REQUIRE_FALSE(invalid);
            CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
        }
    };
    const auto accumulator = *make_accumulator(
        {.strike = 100.0, .knock_out_level = 120.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 1.0, .effective_date = effective, .expiry_date = expiry});
    check(accumulator, MonteCarloAccumulatorEngine{{64, 73}}, FiniteDifferenceAccumulatorEngine{});
    const auto binary = *make_binary_snowball_option(
        {.knock_out_coupon_rates = {0.1, 0.1}, .maturity_coupon_rate = 0.1, .knock_out_levels = {120.0, 120.0}, .observation_dates = {valuation, expiry}, .barrier_state = AutocallableBarrierState::none, .effective_date = effective, .expiry_date = expiry});
    check(binary, MonteCarloBinarySnowballEngine{{64, 73}}, FiniteDifferenceBinarySnowballEngine{});
    const auto ternary = *make_ternary_snowball_option(
        {.knock_out_coupon_rates = {0.1, 0.1}, .maturity_coupon_rate = 0.1, .minimum_coupon_rate = 0.0, .knock_in_level = 70.0, .knock_out_levels = {120.0, 120.0}, .observation_dates = {valuation, expiry}, .barrier_state = AutocallableBarrierState::none, .effective_date = effective, .expiry_date = expiry});
    check(ternary, MonteCarloTernarySnowballEngine{{64, 73}}, FiniteDifferenceTernarySnowballEngine{});
    const auto snowball = *make_snowball_option(
        {.knock_out_coupon_rates = {0.1, 0.1}, .maturity_coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 70.0, .knock_out_levels = {120.0, 120.0}, .upper_strike = 100.0, .lower_strike = 0.0, .observation_dates = {valuation, expiry}, .barrier_state = AutocallableBarrierState::none, .effective_date = effective, .expiry_date = expiry});
    check(snowball, MonteCarloSnowballEngine{{64, 73}}, FiniteDifferenceSnowballEngine{});
    const auto phoenix = *make_phoenix_option(
        {.coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 70.0, .knock_out_levels = {120.0, 120.0}, .coupon_barrier_levels = {90.0, 90.0}, .upper_strike = 100.0, .lower_strike = 0.0, .observation_dates = {valuation, expiry}, .barrier_state = AutocallableBarrierState::none, .effective_date = effective, .expiry_date = expiry});
    check(phoenix, MonteCarloPhoenixEngine{{64, 73}}, FiniteDifferencePhoenixEngine{});
    const auto settled = *make_binary_snowball_option(
        {.knock_out_coupon_rates = {0.1, 0.1}, .maturity_coupon_rate = 0.1, .knock_out_levels = {120.0, 120.0}, .observation_dates = {day(2025, 3, 3), expiry}, .barrier_state = AutocallableBarrierState::knocked_out, .effective_date = effective, .expiry_date = expiry});
    check(settled, MonteCarloBinarySnowballEngine{{64, 73}}, FiniteDifferenceBinarySnowballEngine{});
}

TEST_CASE("Implied solvers reject known unidentifiable parameters", "[pricing-api]")
{
    const auto market = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 110.0, expiry);
    const auto call = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, day(2025, 1, 1), expiry);
    const auto volatility = kiyosi::implied_volatility(
        kiyosi::AnalyticVanillaEngine{}, call, market, 10.0);
    REQUIRE_FALSE(volatility);
    CHECK(volatility.error().category == kiyosi::ErrorCategory::unsupported_operation);

    const auto note = *kiyosi::make_binary_snowball_option(
        {.knock_out_coupon_rates = {0.1, 0.1}, .maturity_coupon_rate = 0.05, .knock_out_levels = {120.0, 120.0}, .observation_dates = {day(2025, 7, 1), expiry}, .barrier_state = kiyosi::AutocallableBarrierState::knocked_out, .effective_date = day(2025, 1, 1), .expiry_date = expiry});
    const auto coupon = kiyosi::implied_coupon(
        kiyosi::MonteCarloBinarySnowballEngine{{32, 1}}, note, market, 0.0,
        kiyosi::CouponQuoteConvention::shift_maturity_coupon);
    REQUIRE_FALSE(coupon);
    CHECK(coupon.error().category == kiyosi::ErrorCategory::unsupported_operation);

    const auto note_volatility = kiyosi::implied_volatility(
        kiyosi::MonteCarloBinarySnowballEngine{{32, 1}}, note, market, 0.0);
    REQUIRE_FALSE(note_volatility);
    CHECK(note_volatility.error().category == kiyosi::ErrorCategory::unsupported_operation);

    const auto barrier = *kiyosi::make_barrier_option(
        {.option_type = kiyosi::OptionType::call, .strike = 100.0, .effective_date = day(2025, 1, 1), .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = kiyosi::BarrierType::up_and_out, .touch_state = kiyosi::BarrierTouchState::touched});
    const auto barrier_volatility = kiyosi::implied_volatility(
        kiyosi::AnalyticBarrierEngine{}, barrier, market, 0.0);
    REQUIRE_FALSE(barrier_volatility);
    CHECK(barrier_volatility.error().category == kiyosi::ErrorCategory::unsupported_operation);
}

} // namespace
