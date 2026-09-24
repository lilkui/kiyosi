#include <cmath>
#include <exception>
#include <string>
#include <utility>

#include <benchmark/benchmark.h>
#include <ql/exercise.hpp>
#include <ql/instruments/asianoption.hpp>
#include <ql/instruments/barrieroption.hpp>
#include <ql/instruments/vanillaoption.hpp>
#include <ql/methods/lattices/binomialtree.hpp>
#include <ql/pricingengines/asian/analytic_cont_geom_av_price.hpp>
#include <ql/pricingengines/asian/continuousarithmeticasianlevyengine.hpp>
#include <ql/pricingengines/barrier/analyticbarrierengine.hpp>
#include <ql/pricingengines/barrier/analyticbinarybarrierengine.hpp>
#include <ql/pricingengines/barrier/fdblackscholesbarrierengine.hpp>
#include <ql/pricingengines/vanilla/analyticeuropeanengine.hpp>
#include <ql/pricingengines/vanilla/binomialengine.hpp>
#include <ql/pricingengines/vanilla/bjerksundstenslandengine.hpp>
#include <ql/pricingengines/vanilla/fdblackscholesvanillaengine.hpp>
#include <ql/pricingengines/vanilla/integralengine.hpp>
#include <ql/pricingengines/vanilla/mcamericanengine.hpp>
#include <ql/pricingengines/vanilla/mceuropeanengine.hpp>
#include <ql/processes/blackscholesprocess.hpp>
#include <ql/quotes/simplequote.hpp>
#include <ql/settings.hpp>
#include <ql/termstructures/volatility/equityfx/blackconstantvol.hpp>
#include <ql/termstructures/yield/flatforward.hpp>
#include <ql/time/calendars/nullcalendar.hpp>
#include <ql/time/daycounters/actual365fixed.hpp>

namespace {

template <typename Price>
void register_quantlib(const char* name, Price price)
{
    const std::string full_name = std::string{"matrix/quantlib/"} + name;
    benchmark::RegisterBenchmark(full_name.c_str(), [price = std::move(price)](benchmark::State& state) {
        try {
            const double warmup_price = price();
            if (!std::isfinite(warmup_price)) {
                state.SkipWithError("QuantLib returned a non-finite warmup price");
                return;
            }
            state.counters["price"] = warmup_price;
            for (auto _ : state) {
                double result = price();
                benchmark::DoNotOptimize(result);
            }
        } catch (const std::exception& error) {
            state.SkipWithError(error.what());
        }
    });
}

template <typename Instrument>
void register_contract(const char* name, QuantLib::ext::shared_ptr<Instrument> contract,
                       QuantLib::ext::shared_ptr<QuantLib::PricingEngine> engine)
{
    contract->setPricingEngine(std::move(engine));
    register_quantlib(name, [contract = std::move(contract)] {
        // Instrument::NPV() caches its result; recalculate() forces a real pricing call.
        contract->recalculate();
        return contract->NPV();
    });
}

bool register_matrix()
{
    using namespace QuantLib;
    const Date start{1, January, 2025};
    const Date expiry{1, January, 2026};
    Settings::instance().evaluationDate() = start;
    const DayCounter day_count = Actual365Fixed{};
    const auto spot = Handle<Quote>{ext::make_shared<SimpleQuote>(100.0)};
    const auto dividend = Handle<YieldTermStructure>{ext::make_shared<FlatForward>(start, 0.01, day_count)};
    const auto risk_free = Handle<YieldTermStructure>{ext::make_shared<FlatForward>(start, 0.04, day_count)};
    const auto volatility = Handle<BlackVolTermStructure>{
        ext::make_shared<BlackConstantVol>(start, NullCalendar{}, 0.2, day_count)};
    const auto process = ext::make_shared<BlackScholesMertonProcess>(spot, dividend, risk_free, volatility);
    const auto european_exercise = ext::make_shared<EuropeanExercise>(expiry);
    const auto american_exercise = ext::make_shared<AmericanExercise>(start, expiry);
    const auto binary_exercise = ext::make_shared<AmericanExercise>(start, expiry, true);
    const auto call = ext::make_shared<PlainVanillaPayoff>(Option::Call, 100.0);
    const auto put = ext::make_shared<PlainVanillaPayoff>(Option::Put, 100.0);
    const auto cash = ext::make_shared<CashOrNothingPayoff>(Option::Call, 100.0, 10.0);
    const auto asset = ext::make_shared<AssetOrNothingPayoff>(Option::Call, 100.0);

    const auto european = [&] { return ext::make_shared<VanillaOption>(call, european_exercise); };
    const auto american = [&] { return ext::make_shared<VanillaOption>(put, american_exercise); };
    const auto cash_digital = [&] { return ext::make_shared<VanillaOption>(cash, european_exercise); };
    const auto asset_digital = [&] { return ext::make_shared<VanillaOption>(asset, european_exercise); };
    const auto barrier = [&] {
        return ext::make_shared<BarrierOption>(Barrier::DownOut, 80.0, 0.0, call, european_exercise);
    };
    const auto cash_binary = [&] {
        return ext::make_shared<BarrierOption>(Barrier::DownOut, 80.0, 0.0, cash, binary_exercise);
    };
    const auto asset_binary = [&] {
        return ext::make_shared<BarrierOption>(Barrier::DownOut, 80.0, 0.0, asset, binary_exercise);
    };

    register_contract("european/analytic", european(), ext::make_shared<AnalyticEuropeanEngine>(process));
    register_contract("european/crr", european(),
                      ext::make_shared<BinomialVanillaEngine<CoxRossRubinstein>>(process, 128));
    register_contract("european/fd", european(),
                      ext::make_shared<FdBlackScholesVanillaEngine>(process, 80, 80));
    register_contract("european/quadrature", european(), ext::make_shared<IntegralEngine>(process));
    register_contract("european/mc", european(),
                      MakeMCEuropeanEngine<PseudoRandom>(process).withSteps(10).withSamples(2'500).withAntitheticVariate().withSeed(42));
    register_contract("american/bjerksund_stensland", american(),
                      ext::make_shared<BjerksundStenslandApproximationEngine>(process));
    register_contract("american/crr", american(),
                      ext::make_shared<BinomialVanillaEngine<CoxRossRubinstein>>(process, 128));
    register_contract("american/fd", american(),
                      ext::make_shared<FdBlackScholesVanillaEngine>(process, 80, 80));
    register_contract("american/mc", american(),
                      MakeMCAmericanEngine<PseudoRandom>(process).withSteps(20).withSamples(2'500).withAntitheticVariate().withSeed(42));
    register_contract("cash_digital/analytic", cash_digital(), ext::make_shared<AnalyticEuropeanEngine>(process));
    register_contract("cash_digital/fd", cash_digital(),
                      ext::make_shared<FdBlackScholesVanillaEngine>(process, 80, 80));
    register_contract("cash_digital/quadrature", cash_digital(), ext::make_shared<IntegralEngine>(process));
    register_contract("asset_digital/analytic", asset_digital(), ext::make_shared<AnalyticEuropeanEngine>(process));
    register_contract("asset_digital/fd", asset_digital(),
                      ext::make_shared<FdBlackScholesVanillaEngine>(process, 80, 80));
    register_contract("asset_digital/quadrature", asset_digital(), ext::make_shared<IntegralEngine>(process));
    register_contract("barrier/analytic", barrier(), ext::make_shared<AnalyticBarrierEngine>(process));
    register_contract("barrier/fd", barrier(), ext::make_shared<FdBlackScholesBarrierEngine>(process, 80, 80));
    register_contract("cash_binary_barrier/analytic", cash_binary(),
                      ext::make_shared<AnalyticBinaryBarrierEngine>(process));
    register_contract("asset_binary_barrier/analytic", asset_binary(),
                      ext::make_shared<AnalyticBinaryBarrierEngine>(process));

    for (const bool pays_asset : {false, true}) {
        const auto touch_payoff = [&](Option::Type side) -> ext::shared_ptr<StrikedTypePayoff> {
            if (pays_asset) return ext::make_shared<AssetOrNothingPayoff>(side, 120.0);
            return ext::make_shared<CashOrNothingPayoff>(side, 120.0, 10.0);
        };
        const auto call_touch = ext::make_shared<BarrierOption>(
            Barrier::UpIn, 120.0, 0.0, touch_payoff(Option::Call), binary_exercise);
        const auto put_touch = ext::make_shared<BarrierOption>(
            Barrier::UpIn, 120.0, 0.0, touch_payoff(Option::Put), binary_exercise);
        call_touch->setPricingEngine(ext::make_shared<AnalyticBinaryBarrierEngine>(process));
        put_touch->setPricingEngine(ext::make_shared<AnalyticBinaryBarrierEngine>(process));
        register_quantlib(pays_asset ? "asset_touch/analytic" : "cash_touch/analytic",
                          [call_touch, put_touch] {
                              call_touch->recalculate();
                              put_touch->recalculate();
                              return call_touch->NPV() + put_touch->NPV();
                          });
    }

    register_contract("geometric_asian/analytic",
                      ext::make_shared<ContinuousAveragingAsianOption>(Average::Geometric, call, european_exercise),
                      ext::make_shared<AnalyticContinuousGeometricAveragePriceAsianEngine>(process));
    register_contract("arithmetic_asian/approximation",
                      ext::make_shared<ContinuousAveragingAsianOption>(Average::Arithmetic, start, call,
                                                                       european_exercise),
                      ext::make_shared<ContinuousArithmeticAsianLevyEngine>(
                          process, Handle<Quote>{ext::make_shared<SimpleQuote>(100.0)}));
    return true;
}

[[maybe_unused]] const bool matrix_registered = register_matrix();

} // namespace
