// Calculator Extension for Raycast Clone
// Provides basic calculator functionality with expression evaluation

function evaluateExpression(expression) {
    try {
        // Simple validation - only allow numbers and basic operators
        var cleanExpression = expression.replace(/\s/g, '');
        
        // Basic validation - only allow numbers, operators, and parentheses
        if (!/^[0-9+\-*/().]+$/.test(cleanExpression)) {
            return null;
        }
        
        // Don't allow consecutive operators
        if (/[+\-*/]{2,}/.test(cleanExpression)) {
            return null;
        }
        
        // Simple manual evaluation for basic expressions
        // For now, let's support simple expressions like "2+2", "10*5", etc.
        if (cleanExpression.indexOf('+') !== -1) {
            var parts = cleanExpression.split('+');
            if (parts.length === 2) {
                return parseFloat(parts[0]) + parseFloat(parts[1]);
            }
        } else if (cleanExpression.indexOf('-') !== -1) {
            var parts = cleanExpression.split('-');
            if (parts.length === 2) {
                return parseFloat(parts[0]) - parseFloat(parts[1]);
            }
        } else if (cleanExpression.indexOf('*') !== -1) {
            var parts = cleanExpression.split('*');
            if (parts.length === 2) {
                return parseFloat(parts[0]) * parseFloat(parts[1]);
            }
        } else if (cleanExpression.indexOf('/') !== -1) {
            var parts = cleanExpression.split('/');
            if (parts.length === 2) {
                return parseFloat(parts[0]) / parseFloat(parts[1]);
            }
        } else if (cleanExpression.indexOf('(') !== -1 && cleanExpression.indexOf(')') !== -1) {
            // Handle parentheses - simple case
            var inner = cleanExpression.substring(cleanExpression.indexOf('(') + 1, cleanExpression.indexOf(')'));
            var innerResult = evaluateExpression(inner);
            if (innerResult !== null) {
                var outer = cleanExpression.replace(/\([^)]*\)/, innerResult.toString());
                return evaluateExpression(outer);
            }
        }
        
        // If it's just a number, return it
        if (!isNaN(parseFloat(cleanExpression))) {
            return parseFloat(cleanExpression);
        }
        
        return null;
    } catch (error) {
        return null;
    }
}

function formatNumber(num) {
    // Format the number nicely - limit decimal places for readability
    if (num === Math.floor(num)) {
        return num.toString();
    } else {
        return parseFloat(num.toFixed(8)).toString();
    }
}

function onSearch(query) {
    console.log('Calculator onSearch called with query:', query);
    
    if (!query || query === '') {
        // Show calculator help when no query
        var results = [{
            title: 'Calculator',
            subtitle: 'Type a mathematical expression to calculate',
            icon: '🧮',
            action: 'calculator-help'
        }];
        
        console.log('Empty query, showing help');
        if (typeof globalThis.raycast !== 'undefined') {
            console.log('Calling globalThis.raycast.updateList with:', results);
            globalThis.raycast.updateList(results);
        } else {
            console.log('globalThis.raycast is undefined');
        }
        return;
    }
    
    // Try to evaluate the expression
    var result = evaluateExpression(query);
    console.log('Expression evaluation result:', result);
    
    if (result !== null) {
        // Show the calculation result
        var results = [{
            title: formatNumber(result),
            subtitle: '= ' + query,
            icon: '🧮',
            action: 'calculator-result:' + result.toString()
        }];
        
        // Also show the original expression as a secondary result
        if (query !== formatNumber(result)) {
            results.push({
                title: 'Copy Expression',
                subtitle: query,
                icon: '📋',
                action: 'calculator-copy:' + query
            });
        }
        
        console.log('Valid result, showing results:', results);
        if (typeof globalThis.raycast !== 'undefined') {
            console.log('Calling globalThis.raycast.updateList with:', results);
            globalThis.raycast.updateList(results);
        } else {
            console.log('globalThis.raycast is undefined');
        }
    } else {
        // Show error message for invalid expressions
        var results = [{
            title: 'Invalid Expression',
            subtitle: 'Please enter a valid mathematical expression',
            icon: '❌',
            action: 'calculator-error'
        }];
        
        console.log('Invalid expression, showing error');
        if (typeof globalThis.raycast !== 'undefined') {
            console.log('Calling globalThis.raycast.updateList with:', results);
            globalThis.raycast.updateList(results);
        } else {
            console.log('globalThis.raycast is undefined');
        }
    }
}

function onAction(action, itemId) {
    console.log('Calculator onAction called with action:', action, 'itemId:', itemId);
    
    if (action.startsWith('calculator-result:')) {
        let result = action.split(':')[1];
        console.log('Calculator result action, result:', result);
        // The result is already copied to clipboard by the main app
        console.log('Result copied to clipboard:', result);
    } else if (action.startsWith('calculator-copy:')) {
        let expression = action.split(':')[1];
        console.log('Calculator copy action, expression:', expression);
        // The expression is already copied to clipboard by the main app
        console.log('Expression copied to clipboard:', expression);
    } else if (action === 'calculator-help') {
        console.log('Calculator help action');
    } else if (action === 'calculator-error') {
        console.log('Calculator error action');
    }
}

// Make the functions available globally
globalThis.onSearch = onSearch;
globalThis.onAction = onAction;