#!/usr/bin/env node

// Test script for calculator extension
const fs = require('fs');
const path = require('path');

// Read the calculator extension
const calculatorCode = fs.readFileSync(path.join(__dirname, 'extensions/calculator.js'), 'utf8');

// Mock the globalThis.raycast object
const mockResults = [];
global.globalThis = {
  raycast: {
    updateList: function(results) {
      mockResults.push(...results);
      console.log('Extension results:', JSON.stringify(results, null, 2));
    }
  }
};

// Set globalThis globally
global.globalThis = global;

// Execute the calculator code
eval(calculatorCode);

// Test various calculations
console.log('\n=== Testing Calculator Extension ===\n');

console.log('Test 1: Simple calculation "2 + 2"');
globalThis.onSearch('2 + 2');
console.log('');

console.log('Test 2: Complex calculation "(10 * 5) / 2"');
globalThis.onSearch('(10 * 5) / 2');
console.log('');

console.log('Test 3: Empty query (should show help)');
globalThis.onSearch('');
console.log('');

console.log('Test 4: Invalid expression "abc + def"');
globalThis.onSearch('abc + def');
console.log('');

console.log('Test 5: Decimal calculation "3.14 * 2"');
globalThis.onSearch('3.14 * 2');

console.log('\n=== Test Complete ===');
console.log(`Total results generated: ${mockResults.length}`);