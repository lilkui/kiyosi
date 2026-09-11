#pragma once

#include <cstdint>
#include <optional>
#include <kiyosi/instruments/structured.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

struct StructuredMonteCarloSettings {
    int path_count = 20'000;
    std::optional<std::uint64_t> seed = 1;
};

template <typename Option>
class KIYOSI_EXPORT MonteCarloStructuredEngine {
public:
    explicit MonteCarloStructuredEngine(StructuredMonteCarloSettings settings = {}) : settings_(settings) {}
    explicit MonteCarloStructuredEngine(int path_count, std::optional<std::uint64_t> seed = 1)
        : settings_{path_count, seed} {}
    [[nodiscard]] result<PricingResult> price(const Option&, const PricingContext&) const;
    StructuredMonteCarloSettings settings() const noexcept { return settings_; }

private:
    StructuredMonteCarloSettings settings_;
};

using MonteCarloAccumulatorEngine = MonteCarloStructuredEngine<Accumulator>;
using MonteCarloPhoenixEngine = MonteCarloStructuredEngine<PhoenixOption>;
using MonteCarloSnowballEngine = MonteCarloStructuredEngine<SnowballOption>;
using MonteCarloBinarySnowballEngine = MonteCarloStructuredEngine<BinarySnowballOption>;
using MonteCarloTernarySnowballEngine = MonteCarloStructuredEngine<TernarySnowballOption>;


} // namespace kiyosi
