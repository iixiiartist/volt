def single_number(nums):
    seen = {}
    for n in nums:
        if n in seen:
            seen[n] += 1
        else:
            seen[n] = 1
    for n, count in seen.items():
        if count == 1:
            return n

print(single_number([2,2,1]))
print(single_number([4,1,2,1,2]))
print(single_number([1]))