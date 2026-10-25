#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>

#include "monte_carlo_simulation.hpp"
#include "monte_carlo_digital.hpp"
#include "monte_carlo_barrier.hpp"
#include "monte_carlo_asian.hpp"

namespace kiyosi::detail {

enum class CudaPricingStatus : unsigned char {
    success,
    unavailable,
    failure,
    out_of_memory,
    invalid_result,
};

struct CudaEuropeanRequest {
    int path_count{};
    std::uint64_t seed{};
    double spot{};
    double strike{};
    double drift{};
    double diffusion{};
    int payoff_sign{};
    bool antithetic{};
};

struct CudaAmericanRequest {
    int path_count{};
    int step_count{};
    std::uint64_t seed{};
    double spot{};
    double strike{};
    double drift{};
    double diffusion{};
    double discount{};
    int payoff_sign{};
    bool antithetic{};
};

struct CudaDigitalRequest {
    int path_count{};
    std::uint64_t seed{};
    DigitalMonteCarloParameters parameters{};
    bool antithetic{};
    bool delta{};
    bool gamma{};
    int measure{};
};

struct CudaAsianRequest {
    int path_count{};
    std::uint64_t seed{};
    AsianMonteCarloParameters parameters{};
    bool antithetic{};
    bool delta{};
    bool gamma{};
    int measure{};
};

struct CudaBarrierRequest {
    int path_count{};
    std::uint64_t seed{};
    BarrierMonteCarloParameters parameters{};
    bool antithetic{};
    bool delta{};
    bool gamma{};
    int measure{};
};

struct CudaAccumulatorRequest {
    int path_count{};
    std::uint64_t seed{};
    double spot{};
    double strike{};
    double knock_out{};
    double daily_quantity{};
    double acceleration{};
    double initial_quantity{};
    bool antithetic{};
};

struct CudaAutocallableRequest {
    int path_count{};
    std::uint64_t seed{};
    double spot{};
    MonteCarloDiscount terminal_discount{};
    AutocallableProgram program{};
    AutocallablePathState initial_state{};
    AutocallableProgram alternate_program{};
    AutocallablePathState alternate_initial_state{};
    bool antithetic{};
};

struct CudaPricingResult {
    CudaPricingStatus status{};
    double payoff_mean{};
    const char* message{};
    double alternate_payoff_mean{};
};

struct CudaSpotPricingResult {
    CudaPricingResult result{};
    std::array<double, 5> payoff_means{};
};

// One operation owns this workspace; it must not be shared across concurrent calls.
// Resources are created lazily so settlements still work without a CUDA device.
class CudaWorkspace {
public:
    struct Impl;
    CudaWorkspace();
    ~CudaWorkspace();
    CudaWorkspace(const CudaWorkspace&) = delete;
    CudaWorkspace& operator=(const CudaWorkspace&) = delete;
    Impl& storage();

private:
    std::unique_ptr<Impl> impl_;
};

[[nodiscard]] CudaPricingResult cuda_european_price(CudaEuropeanRequest request, CudaWorkspace* workspace = nullptr);
// Output slots are price, delta and gamma; unrequested Greek slots are zero.
[[nodiscard]] CudaSpotPricingResult cuda_digital_prices(CudaDigitalRequest request, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaSpotPricingResult cuda_asian_prices(CudaAsianRequest request, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaSpotPricingResult cuda_barrier_prices(CudaBarrierRequest request,
                                                        std::span<const BarrierMonteCarloStep> steps,
                                                        CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaPricingResult cuda_american_price(
    CudaAmericanRequest request, CudaWorkspace* workspace = nullptr, bool cache_growth = false);
[[nodiscard]] CudaPricingResult cuda_accumulator_price(
    CudaAccumulatorRequest request, std::span<const SimulationStep> steps, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaPricingResult cuda_autocallable_price(
    CudaAutocallableRequest request, std::span<const AutocallableStep> steps, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaPricingResult cuda_autocallable_coupon_prices(
    CudaAutocallableRequest request, std::span<const AutocallableStep> steps, CudaWorkspace* workspace = nullptr);
// Internal batches contain 1..5 requests with identical path counts, seeds and simulation steps.
[[nodiscard]] CudaSpotPricingResult cuda_accumulator_spot_prices(
    std::span<const CudaAccumulatorRequest> requests, std::span<const SimulationStep> steps, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaSpotPricingResult cuda_autocallable_spot_prices(
    std::span<const CudaAutocallableRequest> requests, std::span<const AutocallableStep> steps, CudaWorkspace* workspace = nullptr);

} // namespace kiyosi::detail
