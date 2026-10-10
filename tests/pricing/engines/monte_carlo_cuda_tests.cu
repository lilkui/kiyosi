#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <cuda_runtime.h>

#include <algorithm>
#include <limits>
#include <memory>
#include <vector>

#include "pricing/engines/monte_carlo_cuda_reduction.hpp"

namespace {
struct DeviceDeleter {
    void operator()(void* pointer) const { cudaFree(pointer); }
};
} // namespace

TEST_CASE("CUDA payoff reduction preserves weights and extreme values across blocks", "[cuda][monte-carlo-performance]")
{
    using namespace kiyosi::detail;
    const auto allocate = [](std::size_t bytes) {
        void* pointer = nullptr;
        REQUIRE(cudaMalloc(&pointer, bytes) == cudaSuccess);
        return std::unique_ptr<void, DeviceDeleter>{pointer};
    };
    const auto input = allocate(100'003 * sizeof(double));
    const auto partials = allocate(256 * sizeof(MonteCarloMean));
    const auto output = allocate(sizeof(double));
    const auto reduce = [&](const std::vector<double>& values) {
        const int count = static_cast<int>(values.size());
        const int blocks = std::min((count + monte_carlo_reduction_threads - 1) / monte_carlo_reduction_threads, 256);
        REQUIRE(cudaMemcpy(input.get(), values.data(), values.size() * sizeof(double), cudaMemcpyHostToDevice) == cudaSuccess);
        reduce_payoffs<<<blocks, monte_carlo_reduction_threads>>>(static_cast<const double*>(input.get()), count,
                                                                  static_cast<MonteCarloMean*>(partials.get()));
        REQUIRE(cudaGetLastError() == cudaSuccess);
        reduce_payoffs<<<1, monte_carlo_reduction_threads>>>(static_cast<const MonteCarloMean*>(partials.get()), blocks,
                                                             static_cast<double*>(output.get()));
        REQUIRE(cudaGetLastError() == cudaSuccess);
        double result = 0.0;
        REQUIRE(cudaMemcpy(&result, output.get(), sizeof(double), cudaMemcpyDeviceToHost) == cudaSuccess);
        return result;
    };
    for (const int count : {1, 255, 256, 257, 65'535, 65'536, 65'537, 100'003}) {
        CAPTURE(count);
        std::vector<double> values(static_cast<std::size_t>(count));
        double sum = 0.0;
        for (int index = 0; index < count; ++index) {
            values[static_cast<std::size_t>(index)] = index % 13 - 6.0;
            sum += values[static_cast<std::size_t>(index)];
        }
        CHECK(reduce(values) == Catch::Approx(sum / count).margin(1e-14));
    }
    const double maximum = std::numeric_limits<double>::max();
    for (const double value : {maximum, -maximum, std::numeric_limits<double>::denorm_min()}) {
        CAPTURE(value);
        CHECK(reduce(std::vector<double>(100'003, value)) == value);
    }
    std::vector<double> cancellation(100'002);
    for (std::size_t index = 0; index < cancellation.size(); ++index)
        cancellation[index] = index % 2 == 0 ? maximum : -maximum;
    CHECK(reduce(cancellation) == 0.0);
}
