def majority_element(nums):
    count = 0
    candidate = None
    for n in nums:
        if count == 0:
            candidate = n
        count += (1 if n == candidate else -1)
    return candidate

print(majority_element([3,2,3]))
print(majority_element([2,2,1,1,1,2,2]))