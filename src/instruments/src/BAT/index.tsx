import './style.scss';
import React from 'react';
import { render } from '../Common';
import { useSimVar } from '../Common/simVars';

const BatDisplay = ({ number, x, y }) => {
    const [ltsTest] = useSimVar('L:XMLVAR_LTS_Test', 'Bool', 200);
    const [voltage] = useSimVar(`L:A32NX_ELEC_BAT_${number}_POTENTIAL`, 'Volts', 200);

    return (
        <text x={x} y={y}>{ltsTest ? '88.8V' : `${voltage.toFixed(1)}V`}</text>
    );
};

const BatRoot = () => (
    <svg className="bat-svg" viewBox="0 0 200 100">
        <BatDisplay number={1} x="184" y="45" />
        <BatDisplay number={2} x="184" y="95" />
    </svg>
);

render(<BatRoot />);
