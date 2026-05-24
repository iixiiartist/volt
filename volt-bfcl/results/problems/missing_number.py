def missing_number(nums):
    n = len(nums)
    return n * (n + 1) // 2 - sum(nums)

print(missing_number([3,0,1]))
print(missing_number([0,1]))
print(missing_number([9,6,4,2,3,5,7,0,1]))